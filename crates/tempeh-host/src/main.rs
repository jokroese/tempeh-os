use std::collections::VecDeque;
use std::convert::Infallible;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::{Json, Router, routing::get};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tempeh_control::{
    ControlLoop, ControlReading, Controller, Heater, HeaterError, TraceThermometer,
    parse_temperature_line, run_trace_control,
};
use tempeh_model::{EnvironmentState, TemperatureReading};
use tempeh_pet::{PetEvent, PetReport, format_event_time, report_for_samples};
use tempeh_runtime::{LatestTemperatureReadings, RealRunConfig, RealRunController};
use tempeh_sim::{SimConfig, Simulator, TemperatureTrace};
use tokio::sync::broadcast;

const DEFAULT_SERIAL_BAUD: u32 = 115_200;
const DEFAULT_LIVE_ADDR: &str = "127.0.0.1:8787";
const LIVE_RING_CAPACITY: usize = 10_800;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = env::args().nth(1).unwrap_or_else(|| "html".to_string());
    let config = SimConfig::default();
    let samples = run_closed_loop_simulation(config);

    match command.as_str() {
        "csv" => print_csv(&samples),
        "html" | "view" => {
            let out_path = Path::new("out/sim.html");
            fs::create_dir_all("out")?;
            fs::write(out_path, render_html(&samples, &config))?;
            println!("Wrote {}", out_path.display());
            println!("Open it in a browser.");
        }
        "control" => {
            let trace = TemperatureTrace::from_box_air_states(samples.iter().copied());
            let run = run_trace_control(config.controller, trace, config.dt_s, config.duration_s)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            print_control_csv(&run.readings);
        }
        "pet" => {
            print_pet_report(&samples, &config);
        }
        "plug-test" => {
            run_plug_test(env::args().nth(2))?;
        }
        "trace-control-test" => {
            run_trace_control_test(env::args().nth(2))?;
        }
        "thermometer-test" => {
            run_thermometer_test(env::args().nth(2))?;
        }
        "real-control-test" => {
            run_real_control_test(env::args().nth(2), env::args().nth(3), env::args().nth(4))?;
        }
        "real-control-live" => {
            run_real_control_live(env::args().nth(2), env::args().nth(3), env::args().nth(4))?;
        }
        "ports" | "list-ports" => {
            list_serial_ports(env::args().any(|arg| arg == "--all"))?;
        }
        "help" | "--help" | "-h" => print_help(),
        other => {
            eprintln!("Unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    }

    Ok(())
}

fn print_help() {
    eprintln!(
        "Usage:\n  cargo run -p tempeh-host -- html                                   # write out/sim.html\n  cargo run -p tempeh-host -- csv                                    # print simulation CSV\n  cargo run -p tempeh-host -- control                                # print simulated control-loop CSV\n  cargo run -p tempeh-host -- pet                                    # print the mycelial pet status\n  cargo run -p tempeh-host -- ports                                  # recommend likely ESP32 serial port\n  cargo run -p tempeh-host -- ports --all                            # list all available serial ports\n  cargo run -p tempeh-host -- plug-test <url>                        # turn Tasmota plug on, wait, turn off\n  cargo run -p tempeh-host -- trace-control-test <url>               # drive Tasmota plug from a short fake temperature trace\n  cargo run -p tempeh-host -- thermometer-test <port|->              # read labelled temperature lines from serial or stdin\n  cargo run -p tempeh-host -- real-control-test <port> <url> [csv]   # read real probe, drive plug, save CSV\n  cargo run -p tempeh-host -- real-control-live <port> <url> [csv]   # real control plus live web UI\n\nEnvironment:\n  TEMPEH_TASMOTA_URL=http://192.168.1.50"
    );
}

fn print_control_csv(readings: &[ControlReading]) {
    println!("{}", ControlReading::csv_header());
    for reading in readings {
        println!("{}", reading.csv_row());
    }
}

fn print_csv(samples: &[EnvironmentState]) {
    println!("{}", EnvironmentState::csv_header());
    for sample in samples {
        println!("{}", sample.csv_row());
    }
}

fn print_pet_report(samples: &[EnvironmentState], config: &SimConfig) {
    let Some(report) = report_for_samples(samples, config.controller) else {
        eprintln!("No samples available.");
        return;
    };

    println!("🍄 Tempeh OS Pet Report");
    println!();
    println!("Name: Miso");
    println!("Mood: {}", report.pet.mood.label());
    println!("Core: {:.1} °C", report.state.tempeh_core_temp_c);
    println!("Box: {:.1} °C", report.state.box_air_temp_c);
    println!(
        "Progress: {:.0}%",
        report.state.fermentation_progress * 100.0
    );
    println!(
        "Mycelium confidence: {:.0}%",
        report.pet.mycelium_confidence * 100.0
    );
    println!("Safety margin: {:.1} °C", report.pet.safety_margin_c);
    match report.pet.estimated_ready_in_s {
        Some(seconds) => println!("Estimated ready in: {:.1} h", seconds / 3600.0),
        None => println!("Estimated ready in: unknown"),
    }
    println!();
    println!("Miso says:");
    println!("“{}”", report.pet.message());

    if !report.events.is_empty() {
        println!();
        println!("Diary:");
        for event in &report.events {
            println!("{}  {}", format_event_time(*event), event.message());
        }
    }
}

fn render_html(samples: &[EnvironmentState], config: &SimConfig) -> String {
    let final_state = samples.last().copied().unwrap_or_else(|| samples[0]);
    let pet_report = report_for_samples(samples, config.controller);

    let temp_svg = render_line_chart(
        samples,
        "Temperatures",
        "Temperature (°C)",
        900,
        340,
        &[
            SeriesSpec::new("Room", |s| s.room_air_temp_c),
            SeriesSpec::new("Box air", |s| s.box_air_temp_c),
            SeriesSpec::new("Tempeh core", |s| s.tempeh_core_temp_c),
        ],
    );

    let progress_svg = render_line_chart(
        samples,
        "Fermentation progress",
        "Progress / heat",
        900,
        300,
        &[
            SeriesSpec::new("Progress", |s| s.fermentation_progress),
            SeriesSpec::new("Metabolic heat × 10,000", |s| {
                s.metabolic_heat_rate_c_per_s * 10_000.0
            }),
        ],
    );

    let pet_html = pet_report.as_ref().map(render_pet_card).unwrap_or_default();

    let heater_svg = render_heater_chart(samples, 900, 160);

    let rows = samples
        .iter()
        .step_by((samples.len() / 24).max(1))
        .map(|s| {
            format!(
                "<tr><td>{:.1}</td><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td><td>{:.3}</td><td>{:.6}</td><td>{}</td></tr>",
                s.time_s / 3600.0,
                s.room_air_temp_c,
                s.box_air_temp_c,
                s.tempeh_core_temp_c,
                s.fermentation_progress,
                s.metabolic_heat_rate_c_per_s,
                if s.heater_on { "on" } else { "off" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Tempeh OS simulation v0</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root {{
  color-scheme: light dark;
  --bg: #f8f5ee;
  --fg: #211d17;
  --muted: #6b6258;
  --card: #fffaf1;
  --line: #d8ccba;
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --bg: #151311;
    --fg: #eee7dc;
    --muted: #aaa096;
    --card: #201d19;
    --line: #3a332b;
  }}
}}
body {{
  margin: 0;
  font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  background: var(--bg);
  color: var(--fg);
}}
main {{
  max-width: 980px;
  margin: 0 auto;
  padding: 32px 20px 56px;
}}
h1 {{
  font-size: 2rem;
  margin: 0 0 8px;
}}
p {{
  color: var(--muted);
  line-height: 1.5;
}}
.cards {{
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
  gap: 12px;
  margin: 24px 0;
}}
.card {{
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 14px;
  padding: 14px;
}}
.card .label {{
  color: var(--muted);
  font-size: 0.85rem;
}}
.card .value {{
  font-size: 1.45rem;
  font-weight: 700;
  margin-top: 6px;
}}
.chart {{
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 18px;
  padding: 14px;
  margin: 18px 0;
  overflow-x: auto;
}}
svg {{
  max-width: 100%;
  height: auto;
}}
table {{
  border-collapse: collapse;
  width: 100%;
  margin-top: 16px;
  font-variant-numeric: tabular-nums;
}}
th, td {{
  border-bottom: 1px solid var(--line);
  padding: 8px;
  text-align: right;
}}
th:first-child, td:first-child {{
  text-align: left;
}}
code {{
  background: color-mix(in srgb, var(--card) 70%, var(--line));
  padding: 2px 5px;
  border-radius: 6px;
}}
.pet {{
  display: grid;
  grid-template-columns: minmax(120px, 180px) 1fr;
  gap: 18px;
  align-items: center;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 22px;
  padding: 18px;
  margin: 24px 0;
}}
.pet h2 {{
  margin: 0 0 8px;
}}
.pet .speech {{
  color: var(--fg);
  font-size: 1.1rem;
  margin: 8px 0 0;
}}
.pet .stats {{
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
}}
.pet .pill {{
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 5px 9px;
  color: var(--muted);
  font-size: 0.9rem;
}}
.diary {{
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 18px;
  padding: 18px;
  margin: 18px 0 24px;
}}
.diary h2 {{
  margin: 0 0 12px;
}}
.diary ol {{
  list-style: none;
  margin: 0;
  padding: 0;
}}
.diary li {{
  display: grid;
  grid-template-columns: 72px 1fr;
  gap: 12px;
  padding: 8px 0;
  border-top: 1px solid var(--line);
}}
.diary li:first-child {{
  border-top: 0;
}}
.diary time {{
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}}
.mycelium {{
  width: 100%;
  max-width: 180px;
}}
.mycelium .body {{
  transform-origin: 80px 80px;
  animation: pulse 2.8s ease-in-out infinite;
}}
.mycelium.sleepy .body {{
  opacity: 0.55;
}}
.mycelium.warming .body {{
  animation-duration: 1.8s;
}}
.mycelium.thriving .tendril {{
  animation: grow 2.5s ease-in-out infinite alternate;
}}
.mycelium.spicy .body,
.mycelium.panicking .body {{
  animation-duration: 0.8s;
}}
.mycelium.finished .spark {{
  animation: twinkle 1.4s ease-in-out infinite alternate;
}}
@keyframes pulse {{
  0%, 100% {{ transform: scale(1); }}
  50% {{ transform: scale(1.06); }}
}}
@keyframes grow {{
  from {{ stroke-dasharray: 8 12; }}
  to {{ stroke-dasharray: 18 4; }}
}}
@keyframes twinkle {{
  from {{ opacity: 0.35; transform: scale(0.9); }}
  to {{ opacity: 1; transform: scale(1.1); }}
}}
@media (max-width: 620px) {{
  .pet {{
    grid-template-columns: 1fr;
  }}
  .mycelium {{
    max-width: 140px;
  }}
}}
</style>
</head>
<body>
<main>
<h1>Tempeh OS simulation v0</h1>
<p>
This is a first toy model of the incubator: room air, box air, tempeh core,
fermentation progress, metabolic heat, and heater state. This report is generated by composing the
controller with the simulated environment. The model deliberately leaves out vent and fan.
Run <code>cargo run -p tempeh-host -- csv</code> when you want the raw data.
</p>

{pet_html}

<section class="cards">
  <div class="card"><div class="label">Duration</div><div class="value">{duration_h:.1} h</div></div>
  <div class="card"><div class="label">Target box air</div><div class="value">{target:.1} °C</div></div>
  <div class="card"><div class="label">Final box air</div><div class="value">{box_final:.1} °C</div></div>
  <div class="card"><div class="label">Final tempeh core</div><div class="value">{tempeh_final:.1} °C</div></div>
  <div class="card"><div class="label">Fermentation progress</div><div class="value">{progress:.0}%</div></div>
</section>

<div class="chart">{temp_svg}</div>
<div class="chart">{progress_svg}</div>
<div class="chart">{heater_svg}</div>

<h2>Sampled data</h2>
<table>
<thead>
<tr>
<th>time h</th>
<th>room °C</th>
<th>box °C</th>
<th>tempeh °C</th>
<th>progress</th>
<th>heat °C/s</th>
<th>heater</th>
</tr>
</thead>
<tbody>
{rows}
</tbody>
</table>
</main>
</body>
</html>
"#,
        duration_h = config.duration_s / 3600.0,
        target = config.controller.target_box_air_temp_c,
        box_final = final_state.box_air_temp_c,
        tempeh_final = final_state.tempeh_core_temp_c,
        pet_html = pet_html,
        progress = final_state.fermentation_progress * 100.0,
        temp_svg = temp_svg,
        progress_svg = progress_svg,
        heater_svg = heater_svg,
        rows = rows,
    )
}

fn render_pet_card(report: &PetReport) -> String {
    let ready = match report.pet.estimated_ready_in_s {
        Some(seconds) if seconds <= 0.0 => "ready now".to_string(),
        Some(seconds) => format!("{:.1} h to ready", seconds / 3600.0),
        None => "ready time unknown".to_string(),
    };

    let diary = render_diary(&report.events);

    format!(
        r#"<section class="pet">
{svg}
<div>
  <h2>{headline}</h2>
  <p class="speech">“{message}”</p>
  <div class="stats">
    <span class="pill">confidence {confidence:.0}%</span>
    <span class="pill">safety margin {margin:.1} °C</span>
    <span class="pill">{ready}</span>
  </div>
</div>
</section>
{diary}"#,
        svg = render_mycelium_svg(report.pet.mood.css_class()),
        headline = escape_html(&report.pet.headline("Miso")),
        message = escape_html(report.pet.message()),
        confidence = report.pet.mycelium_confidence * 100.0,
        margin = report.pet.safety_margin_c,
        ready = escape_html(&ready),
        diary = diary,
    )
}

fn render_diary(events: &[PetEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }

    let items = events
        .iter()
        .map(|event| {
            format!(
                r#"<li><time>{time}</time><span>{message}</span></li>"#,
                time = escape_html(&format_event_time(*event)),
                message = escape_html(event.message()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<section class="diary">
<h2>Batch diary</h2>
<ol>
{items}
</ol>
</section>"#
    )
}

fn render_mycelium_svg(css_class: &str) -> String {
    format!(
        r##"<svg class="mycelium {css_class}" viewBox="0 0 160 160" role="img" aria-label="Animated mycelium pet">
<defs>
  <radialGradient id="mycelium-body" cx="45%" cy="38%" r="60%">
    <stop offset="0%" stop-color="#fff7df"/>
    <stop offset="60%" stop-color="#f0d7a2"/>
    <stop offset="100%" stop-color="#c69256"/>
  </radialGradient>
</defs>
<g fill="none" stroke="currentColor" opacity="0.45" stroke-linecap="round">
  <path class="tendril" d="M80 82 C42 76 24 56 16 28" stroke-width="3"/>
  <path class="tendril" d="M80 82 C118 76 136 56 144 28" stroke-width="3"/>
  <path class="tendril" d="M80 92 C44 108 30 126 24 148" stroke-width="3"/>
  <path class="tendril" d="M80 92 C116 108 130 126 136 148" stroke-width="3"/>
  <path class="tendril" d="M76 80 C76 52 68 30 54 14" stroke-width="2"/>
  <path class="tendril" d="M84 80 C84 52 92 30 106 14" stroke-width="2"/>
</g>
<g class="body">
  <path d="M42 78 C42 42 65 25 83 27 C105 29 124 50 122 81 C120 113 101 132 78 130 C56 128 42 108 42 78 Z" fill="url(#mycelium-body)" stroke="currentColor" stroke-opacity="0.35" stroke-width="2"/>
  <circle cx="66" cy="76" r="5" fill="#211d17"/>
  <circle cx="96" cy="76" r="5" fill="#211d17"/>
  <path d="M68 96 C76 104 87 104 95 96" fill="none" stroke="#211d17" stroke-width="4" stroke-linecap="round"/>
</g>
<g class="spark" fill="currentColor" opacity="0.55">
  <circle cx="34" cy="36" r="3"/>
  <circle cx="126" cy="44" r="2.5"/>
  <circle cx="116" cy="124" r="3"/>
</g>
</svg>"##,
        css_class = escape_html(css_class)
    )
}

struct SeriesSpec {
    label: &'static str,
    value: fn(&EnvironmentState) -> f32,
}

impl SeriesSpec {
    fn new(label: &'static str, value: fn(&EnvironmentState) -> f32) -> Self {
        Self { label, value }
    }
}

fn render_line_chart(
    samples: &[EnvironmentState],
    title: &str,
    y_label: &str,
    width: u32,
    height: u32,
    series: &[SeriesSpec],
) -> String {
    let margin_left = 64.0;
    let margin_right = 24.0;
    let margin_top = 42.0;
    let margin_bottom = 48.0;

    let plot_w = width as f32 - margin_left - margin_right;
    let plot_h = height as f32 - margin_top - margin_bottom;

    let max_time = samples.last().map(|s| s.time_s).unwrap_or(1.0).max(1.0);

    let mut y_min = f32::INFINITY;
    let mut y_max = f32::NEG_INFINITY;

    for s in samples {
        for spec in series {
            let v = (spec.value)(s);
            y_min = y_min.min(v);
            y_max = y_max.max(v);
        }
    }

    if (y_max - y_min).abs() < 0.001 {
        y_min -= 1.0;
        y_max += 1.0;
    }

    let padding = (y_max - y_min) * 0.08;
    y_min -= padding;
    y_max += padding;

    let x = |time_s: f32| margin_left + (time_s / max_time) * plot_w;
    let y = |value: f32| margin_top + (1.0 - ((value - y_min) / (y_max - y_min))) * plot_h;

    let colours = ["#386cb0", "#fdb462", "#7fc97f", "#ef3b2c", "#984ea3"];

    let mut paths = String::new();
    let mut legend = String::new();

    for (i, spec) in series.iter().enumerate() {
        let colour = colours[i % colours.len()];
        let mut d = String::new();
        for (j, sample) in samples.iter().enumerate() {
            let cmd = if j == 0 { "M" } else { "L" };
            d.push_str(&format!(
                "{} {:.2} {:.2} ",
                cmd,
                x(sample.time_s),
                y((spec.value)(sample))
            ));
        }

        paths.push_str(&format!(
            r#"<path d="{d}" fill="none" stroke="{colour}" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>"#
        ));

        let lx = margin_left + (i as f32) * 180.0;
        let ly = height as f32 - 16.0;
        legend.push_str(&format!(
            r##"<g><rect x="{:.1}" y="{:.1}" width="12" height="12" rx="3" fill="{}"/><text x="{:.1}" y="{:.1}" font-size="13" fill="currentColor">{}</text></g>"##,
            lx,
            ly - 10.0,
            colour,
            lx + 18.0,
            ly,
            escape_html(spec.label),
        ));
    }

    let grid = render_grid(
        width,
        height,
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        y_min,
        y_max,
        max_time,
    );

    format!(
        r##"<svg viewBox="0 0 {width} {height}" role="img" aria-label="{title}">
<title>{title}</title>
<text x="{margin_left}" y="24" font-size="20" font-weight="700" fill="currentColor">{title}</text>
<text x="18" y="{mid_y}" font-size="13" fill="currentColor" transform="rotate(-90 18 {mid_y})">{y_label}</text>
{grid}
{paths}
{legend}
</svg>"##,
        width = width,
        height = height,
        title = escape_html(title),
        y_label = escape_html(y_label),
        margin_left = margin_left,
        mid_y = height as f32 / 2.0,
        grid = grid,
        paths = paths,
        legend = legend,
    )
}

fn render_heater_chart(samples: &[EnvironmentState], width: u32, height: u32) -> String {
    let margin_left = 64.0;
    let margin_right = 24.0;
    let margin_top = 42.0;
    let margin_bottom = 34.0;

    let plot_w = width as f32 - margin_left - margin_right;
    let plot_h = height as f32 - margin_top - margin_bottom;
    let max_time = samples.last().map(|s| s.time_s).unwrap_or(1.0).max(1.0);

    let mut rects = String::new();
    let mut run_start: Option<f32> = None;

    for sample in samples {
        match (sample.heater_on, run_start) {
            (true, None) => run_start = Some(sample.time_s),
            (false, Some(start)) => {
                let x0 = margin_left + (start / max_time) * plot_w;
                let x1 = margin_left + (sample.time_s / max_time) * plot_w;
                rects.push_str(&format!(
                    r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="#fdb462" opacity="0.7"/>"##,
                    x0,
                    margin_top,
                    (x1 - x0).max(1.0),
                    plot_h,
                ));
                run_start = None;
            }
            _ => {}
        }
    }

    if let Some(start) = run_start {
        let x0 = margin_left + (start / max_time) * plot_w;
        let x1 = margin_left + plot_w;
        rects.push_str(&format!(
            r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="#fdb462" opacity="0.7"/>"##,
            x0,
            margin_top,
            (x1 - x0).max(1.0),
            plot_h,
        ));
    }

    let x_axis_y = margin_top + plot_h;

    format!(
        r##"<svg viewBox="0 0 {width} {height}" role="img" aria-label="Heater state">
<title>Heater state</title>
<text x="{margin_left}" y="24" font-size="20" font-weight="700" fill="currentColor">Heater state</text>
<line x1="{margin_left}" y1="{x_axis_y}" x2="{x2}" y2="{x_axis_y}" stroke="currentColor" opacity="0.35"/>
{rects}
<text x="{margin_left}" y="{label_y}" font-size="13" fill="currentColor">orange = heater on</text>
</svg>"##,
        width = width,
        height = height,
        margin_left = margin_left,
        x_axis_y = x_axis_y,
        x2 = width as f32 - margin_right,
        rects = rects,
        label_y = height as f32 - 10.0,
    )
}

fn render_grid(
    width: u32,
    height: u32,
    margin_left: f32,
    margin_right: f32,
    margin_top: f32,
    margin_bottom: f32,
    y_min: f32,
    y_max: f32,
    max_time_s: f32,
) -> String {
    let plot_w = width as f32 - margin_left - margin_right;
    let plot_h = height as f32 - margin_top - margin_bottom;

    let mut out = String::new();

    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let x = margin_left + t * plot_w;
        let hour = (max_time_s * t) / 3600.0;
        out.push_str(&format!(
            r##"<line x1="{x:.1}" y1="{margin_top:.1}" x2="{x:.1}" y2="{y2:.1}" stroke="currentColor" opacity="0.08"/><text x="{x:.1}" y="{label_y:.1}" text-anchor="middle" font-size="12" fill="currentColor">{hour:.0}h</text>"##,
            x = x,
            margin_top = margin_top,
            y2 = margin_top + plot_h,
            label_y = height as f32 - 24.0,
            hour = hour,
        ));
    }

    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let y = margin_top + t * plot_h;
        let value = y_max - t * (y_max - y_min);
        out.push_str(&format!(
            r##"<line x1="{margin_left:.1}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" stroke="currentColor" opacity="0.08"/><text x="{label_x:.1}" y="{text_y:.1}" text-anchor="end" font-size="12" fill="currentColor">{value:.2}</text>"##,
            margin_left = margin_left,
            y = y,
            x2 = width as f32 - margin_right,
            label_x = margin_left - 8.0,
            text_y = y + 4.0,
            value = value,
        ));
    }

    out
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn run_closed_loop_simulation(config: SimConfig) -> Vec<EnvironmentState> {
    let mut simulator = Simulator::new(config);
    let mut controller = Controller::new(config.controller);
    let mut samples = vec![simulator.state()];
    while simulator.state().time_s < config.duration_s {
        let state = simulator.state();
        let heater_on = controller.update(state.box_air_temp_c);
        samples.push(simulator.step(heater_on));
    }
    samples
}

fn tasmota_base_url(url_arg: Option<String>) -> Result<String, std::io::Error> {
    url_arg
        .or_else(|| env::var("TEMPEH_TASMOTA_URL").ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "provide a Tasmota URL, e.g. cargo run -p tempeh-host -- plug-test http://192.168.1.50",
            )
        })
}

#[derive(Debug, Clone)]
struct TasmotaHeater {
    base_url: String,
    heater_on: bool,
}

impl TasmotaHeater {
    fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_base_url(base_url.into()),
            heater_on: false,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn command_url(&self, on: bool) -> String {
        let command = if on { "Power%20On" } else { "Power%20Off" };
        format!("{}/cm?cmnd={command}", self.base_url)
    }
}

impl Heater for TasmotaHeater {
    fn set_heater(&mut self, on: bool) -> Result<(), HeaterError> {
        let url = self.command_url(on);

        ureq::get(&url)
            .call()
            .map_err(|_| HeaterError::CommandFailed)?;

        self.heater_on = on;
        Ok(())
    }

    fn heater_on(&self) -> bool {
        self.heater_on
    }
}

fn normalise_base_url(mut base_url: String) -> String {
    base_url = base_url.trim().trim_end_matches('/').to_string();

    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url
    } else {
        format!("http://{base_url}")
    }
}

fn run_plug_test(url_arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = tasmota_base_url(url_arg)?;
    let mut heater = TasmotaHeater::new(base_url);

    eprintln!("Turning plug on");
    heater
        .set_heater(true)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    eprintln!("Turning plug off");
    heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    eprintln!("Plug test complete at {}", heater.base_url());
    Ok(())
}

fn run_trace_control_test(url_arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = tasmota_base_url(url_arg)?;
    let config = SimConfig::default();

    // This test intentionally uses fake readings. Its purpose is to verify:
    //
    //   TemperatureTrace -> TraceThermometer -> Controller -> TasmotaHeater
    //
    // The values are chosen to produce visible state transitions:
    // - cold readings should turn the heater on
    // - a reading above target + hysteresis should turn it off
    let trace = TemperatureTrace::new(vec![
        20.0,
        21.0,
        config.controller.target_box_air_temp_c + config.controller.hysteresis_c + 1.0,
        config.controller.target_box_air_temp_c,
    ]);

    let thermometer = TraceThermometer::new(trace);
    let heater = TasmotaHeater::new(base_url);
    let mut control = ControlLoop::new(config.controller, thermometer, heater);

    // Start from a known safe state.
    eprintln!(
        "Sending initial off command to {}",
        control.heater().base_url()
    );
    control
        .heater_mut()
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    println!("{}", ControlReading::csv_header());

    let result = run_trace_control_test_steps(&mut control, config.dt_s);

    // Always try to leave the plug off, even if the control sequence failed.
    let shutdown_result = control
        .heater_mut()
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")));

    result?;
    shutdown_result?;

    eprintln!(
        "Trace control test complete. Final command sent: off. Tested plug at {}",
        control.heater().base_url()
    );

    Ok(())
}

fn run_trace_control_test_steps(
    control: &mut ControlLoop<TraceThermometer, TasmotaHeater>,
    dt_s: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    for step in 0..4 {
        let time_s = step as f32 * dt_s;
        let reading = control
            .step(time_s)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        println!("{}", reading.csv_row());

        // Slow enough to see a lamp/plug indicator change.
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct LiveSample {
    seq: u64,
    time_s: f32,
    room_air_temp_c: Option<f32>,
    box_air_temp_c: f32,
    product_temp_c: Option<f32>,
    heater_on: bool,
    reason: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct LiveStatus {
    csv_path: String,
    sample_count: usize,
    retained_sample_count: usize,
    first_retained_seq: Option<u64>,
    latest: Option<LiveSample>,
}

#[derive(Debug)]
struct LiveRunState {
    csv_path: String,
    samples: VecDeque<LiveSample>,
    next_seq: u64,
}

impl LiveRunState {
    fn new(csv_path: impl Into<String>) -> Self {
        Self {
            csv_path: csv_path.into(),
            samples: VecDeque::with_capacity(LIVE_RING_CAPACITY),
            next_seq: 1,
        }
    }

    fn push(
        &mut self,
        time_s: f32,
        room_air_temp_c: Option<f32>,
        box_air_temp_c: f32,
        product_temp_c: Option<f32>,
        heater_on: bool,
        reason: &'static str,
    ) -> LiveSample {
        let sample = LiveSample {
            seq: self.next_seq,
            time_s,
            room_air_temp_c,
            box_air_temp_c,
            product_temp_c,
            heater_on,
            reason,
        };
        self.next_seq = self.next_seq.saturating_add(1);

        if self.samples.len() == LIVE_RING_CAPACITY {
            self.samples.pop_front();
        }

        self.samples.push_back(sample.clone());
        sample
    }

    fn samples_after(&self, after: u64) -> Vec<LiveSample> {
        self.samples
            .iter()
            .filter(|sample| sample.seq > after)
            .cloned()
            .collect()
    }

    fn status(&self) -> LiveStatus {
        LiveStatus {
            csv_path: self.csv_path.clone(),
            sample_count: self.next_seq.saturating_sub(1) as usize,
            retained_sample_count: self.samples.len(),
            first_retained_seq: self.samples.front().map(|sample| sample.seq),
            latest: self.samples.back().cloned(),
        }
    }
}

#[derive(Debug)]
struct LiveAppState {
    run: Mutex<LiveRunState>,
    events: broadcast::Sender<LiveSample>,
}

impl LiveAppState {
    fn new(csv_path: impl Into<String>) -> Self {
        let (events, _receiver) = broadcast::channel(1_024);
        Self {
            run: Mutex::new(LiveRunState::new(csv_path)),
            events,
        }
    }

    fn push_sample(
        &self,
        time_s: f32,
        room_air_temp_c: Option<f32>,
        box_air_temp_c: f32,
        product_temp_c: Option<f32>,
        heater_on: bool,
        reason: &'static str,
    ) {
        let sample = {
            let mut run = self.run.lock().expect("live state mutex poisoned");
            run.push(
                time_s,
                room_air_temp_c,
                box_air_temp_c,
                product_temp_c,
                heater_on,
                reason,
            )
        };

        // It is fine if no browser is currently connected.
        let _ = self.events.send(sample);
    }
}

type SharedLiveAppState = Arc<LiveAppState>;

#[derive(Debug, Deserialize)]
struct SamplesQuery {
    after: Option<u64>,
}

async fn live_index() -> Html<&'static str> {
    Html(LIVE_CONTROL_HTML)
}

async fn live_status(State(state): State<SharedLiveAppState>) -> impl IntoResponse {
    let run = state.run.lock().expect("live state mutex poisoned");
    Json(run.status())
}

async fn live_samples(
    State(state): State<SharedLiveAppState>,
    Query(query): Query<SamplesQuery>,
) -> impl IntoResponse {
    let after = query.after.unwrap_or(0);
    let run = state.run.lock().expect("live state mutex poisoned");
    Json(run.samples_after(after))
}

async fn live_events(
    State(state): State<SharedLiveAppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();

    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(sample) => {
                    let Ok(json) = serde_json::to_string(&sample) else {
                        continue;
                    };

                    yield Ok(Event::default()
                        .event("sample")
                        .id(sample.seq.to_string())
                        .data(json));
                }
                Err(broadcast::error::RecvError::Lagged(_missed)) => {
                    yield Ok(Event::default().event("resync").data("lagged"));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

fn spawn_live_server(
    state: SharedLiveAppState,
    addr: SocketAddr,
) -> thread::JoinHandle<Result<(), String>> {
    thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|error| format!("failed to create Tokio runtime: {error}"))?;

        runtime.block_on(async move {
            let app = Router::new()
                .route("/", get(live_index))
                .route("/api/status", get(live_status))
                .route("/api/samples", get(live_samples))
                .route("/events", get(live_events))
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|error| format!("failed to bind live UI at http://{addr}: {error}"))?;

            axum::serve(listener, app)
                .await
                .map_err(|error| format!("live UI server failed: {error}"))
        })
    })
}

fn run_thermometer_test(source_arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let source = source_arg.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provide a serial port or '-' for stdin, e.g. cargo run -p tempeh-host -- thermometer-test /dev/ttyUSB0",
        )
    })?;
    if source == "-" {
        let stdin = io::stdin();
        let reader = stdin.lock();
        return read_temperature_lines(reader);
    }
    let port = serialport::new(&source, DEFAULT_SERIAL_BAUD)
        .timeout(Duration::from_millis(2_000))
        .open()
        .map_err(|error| {
            std::io::Error::other(format!(
                "failed to open serial port {source} at {DEFAULT_SERIAL_BAUD} baud: {error}"
            ))
        })?;

    eprintln!("Reading temperatures from {source} at {DEFAULT_SERIAL_BAUD} baud.");
    eprintln!("Expected lines from current firmware: temp,box_air,22.437 and temp,room_air,20.125");
    eprintln!("If you see ESP-IDF example logs instead, flash crates/tempeh-firmware-esp32 first.");
    eprintln!("Press Ctrl-C to stop.");
    read_temperature_lines(BufReader::new(port))
}

fn read_temperature_lines<R>(mut reader: R) -> Result<(), Box<dyn std::error::Error>>
where
    R: BufRead,
{
    let start = Instant::now();
    let mut latest = LatestTemperatureReadings::new();
    let mut line = String::new();
    let mut printed_header = false;
    loop {
        line.clear();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if bytes == 0 {
            break;
        }
        let parsed = match parse_temperature_line(&line) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => continue,
            Err(error) => {
                eprintln!(
                    "Ignoring invalid temperature line {:?}: {error:?}",
                    line.trim()
                );
                continue;
            }
        };
        latest.update(parsed.probe, parsed.temp_c);
        let time_s = start.elapsed().as_secs_f32();
        let Some(reading) = latest.reading(time_s) else {
            continue;
        };
        if !printed_header {
            println!("{}", TemperatureReading::csv_header());
            printed_header = true;
        }
        println!("{}", reading.csv_row());
    }
    if !printed_header {
        eprintln!(
            "No complete temperature reading received. Need at least a temp,box_air,<°C> line."
        );
    }
    Ok(())
}

fn run_real_control_test(
    source_arg: Option<String>,
    url_arg: Option<String>,
    csv_arg: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = source_arg.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provide an ESP32 serial port, e.g. cargo run -p tempeh-host -- real-control-test /dev/cu.usbmodem1234561 http://192.168.1.50 out/heat-mat-test.csv",
        )
    })?;
    let base_url = tasmota_base_url(url_arg)?;
    let csv_path = csv_arg.unwrap_or_else(default_real_control_csv_path);
    let port = serialport::new(&source, DEFAULT_SERIAL_BAUD)
        .timeout(Duration::from_millis(2_000))
        .open()
        .map_err(|error| {
            std::io::Error::other(format!(
                "failed to open serial port {source} at {DEFAULT_SERIAL_BAUD} baud: {error}"
            ))
        })?;

    let stop_requested = Arc::new(AtomicBool::new(false));
    {
        let stop_requested = Arc::clone(&stop_requested);
        ctrlc::set_handler(move || {
            stop_requested.store(true, Ordering::SeqCst);
        })
        .map_err(|error| {
            std::io::Error::other(format!("failed to install Ctrl-C handler: {error}"))
        })?;
    }

    let mut heater = TasmotaHeater::new(base_url);
    eprintln!("Starting real control test.");
    eprintln!("Reading box_air from {source} at {DEFAULT_SERIAL_BAUD} baud.");
    eprintln!("Driving Tasmota plug at {}.", heater.base_url());
    eprintln!("Using box_air for control; room_air is logged for context.");
    eprintln!("Saving data to {csv_path}.");
    eprintln!("Press Ctrl-C to stop.");

    let header = "time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason";
    let mut csv_log = CsvLog::create(&csv_path, header)?;

    // Start from a known safe state.
    eprintln!("Sending initial off command.");
    heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    let result = run_real_control_test_loop(
        BufReader::new(port),
        &mut heater,
        Arc::clone(&stop_requested),
        header,
        &mut csv_log,
        None,
    );

    eprintln!("Sending final off command.");
    let shutdown_result = heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")));

    result?;
    shutdown_result?;
    eprintln!("Real control test stopped. Final command sent: off.");
    Ok(())
}

fn run_real_control_live(
    source_arg: Option<String>,
    url_arg: Option<String>,
    csv_arg: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = source_arg.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provide an ESP32 serial port, e.g. cargo run -p tempeh-host -- real-control-live /dev/cu.usbmodem1234561 http://192.168.1.50",
        )
    })?;
    let base_url = tasmota_base_url(url_arg)?;
    let csv_path = csv_arg.unwrap_or_else(default_real_control_csv_path);
    let addr: SocketAddr = DEFAULT_LIVE_ADDR.parse()?;

    let port = serialport::new(&source, DEFAULT_SERIAL_BAUD)
        .timeout(Duration::from_millis(2_000))
        .open()
        .map_err(|error| {
            std::io::Error::other(format!(
                "failed to open serial port {source} at {DEFAULT_SERIAL_BAUD} baud: {error}"
            ))
        })?;

    let stop_requested = Arc::new(AtomicBool::new(false));
    {
        let stop_requested = Arc::clone(&stop_requested);
        ctrlc::set_handler(move || {
            stop_requested.store(true, Ordering::SeqCst);
        })
        .map_err(|error| {
            std::io::Error::other(format!("failed to install Ctrl-C handler: {error}"))
        })?;
    }

    let header = "time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason";
    let mut csv_log = CsvLog::create(&csv_path, header)?;
    let live_state = Arc::new(LiveAppState::new(csv_path.clone()));
    let server_handle = spawn_live_server(Arc::clone(&live_state), addr);

    let mut heater = TasmotaHeater::new(base_url);
    eprintln!("Starting live real control test.");
    eprintln!("Reading box_air from {source} at {DEFAULT_SERIAL_BAUD} baud.");
    eprintln!("Driving Tasmota plug at {}.", heater.base_url());
    eprintln!("Using box_air for control; room_air is logged for context.");
    eprintln!("Saving data to {csv_path}.");
    eprintln!("Live UI: http://{addr}");
    eprintln!("Press Ctrl-C to stop.");

    // Give the server thread a chance to fail fast on bind errors before we start heating.
    thread::sleep(Duration::from_millis(100));
    if server_handle.is_finished() {
        match server_handle.join() {
            Ok(Err(error)) => return Err(std::io::Error::other(error).into()),
            Ok(Ok(())) => {
                return Err(std::io::Error::other("live UI server stopped unexpectedly").into());
            }
            Err(_) => {
                return Err(std::io::Error::other("live UI server thread panicked").into());
            }
        }
    }

    eprintln!("Sending initial off command.");
    heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;

    let result = run_real_control_test_loop(
        BufReader::new(port),
        &mut heater,
        Arc::clone(&stop_requested),
        header,
        &mut csv_log,
        Some(Arc::clone(&live_state)),
    );

    eprintln!("Sending final off command.");
    let shutdown_result = heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")));

    result?;
    shutdown_result?;
    eprintln!("Live real control test stopped. Final command sent: off.");
    Ok(())
}

fn run_real_control_test_loop<R>(
    mut reader: R,
    heater: &mut TasmotaHeater,
    stop_requested: Arc<AtomicBool>,
    header: &str,
    csv_log: &mut CsvLog,
    live_state: Option<SharedLiveAppState>,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: BufRead,
{
    let config = SimConfig::default();
    let mut controller = RealRunController::new(RealRunConfig {
        controller: config.controller,
        ..RealRunConfig::default()
    });
    let start = Instant::now();
    let mut line = String::new();
    let mut printed_header = false;
    let mut latest = LatestTemperatureReadings::new();
    let mut last_heater_on = false;

    while !stop_requested.load(Ordering::SeqCst) {
        line.clear();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if bytes == 0 {
            break;
        }
        let parsed = match parse_temperature_line(&line) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => continue,
            Err(error) => {
                eprintln!(
                    "Ignoring invalid temperature line {:?}: {error:?}",
                    line.trim()
                );
                continue;
            }
        };
        let time_s = start.elapsed().as_secs_f32();
        latest.update_at(time_s, parsed.probe, parsed.temp_c);

        let Some(box_air_temp_c) = latest.box_air_temp_c else {
            continue;
        };
        let product_temp_c = latest.product_temp_c;
        let Some(snapshot) = latest.snapshot_for_update_at(time_s, parsed.probe) else {
            continue;
        };
        let decision = controller.update(snapshot);
        let heater_on = decision.heater_on;
        let reason = decision.reason;

        if heater_on != last_heater_on {
            heater
                .set_heater(heater_on)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            last_heater_on = heater_on;
        }

        if !printed_header {
            println!("{header}");
            printed_header = true;
        }

        let room_air_temp_c = latest.room_air_temp_c;
        let room_air_text = room_air_temp_c
            .map(|temp| format!("{temp:.3}"))
            .unwrap_or_default();
        let product_text = product_temp_c
            .map(|temp| format!("{temp:.3}"))
            .unwrap_or_default();
        let row = format!(
            "{time_s:.0},{room_air_text},{box_air_temp_c:.3},{product_text},{heater_on_int},{reason}",
            heater_on_int = if heater_on { 1 } else { 0 },
        );
        println!("{row}");
        csv_log.write_row(&row)?;

        if let Some(live_state) = live_state.as_ref() {
            live_state.push_sample(
                time_s,
                room_air_temp_c,
                box_air_temp_c,
                product_temp_c,
                heater_on,
                reason,
            );
        }
    }

    if !printed_header {
        eprintln!("No box_air temperature reading received.");
    }
    Ok(())
}

struct CsvLog {
    writer: BufWriter<File>,
}

impl CsvLog {
    fn create(path: impl Into<PathBuf>, header: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "{header}")?;
        writer.flush()?;
        eprintln!("Saving CSV log to {}.", path.display());
        Ok(Self { writer })
    }

    fn write_row(&mut self, row: &str) -> Result<(), Box<dyn std::error::Error>> {
        writeln!(self.writer, "{row}")?;
        // Supervised hardware tests are often stopped with Ctrl-C.
        // Flush every row so the file remains useful after interruption.
        self.writer.flush()?;
        Ok(())
    }
}

fn default_real_control_csv_path() -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("out/real-control-test-{timestamp}.csv")
}

const LIVE_CONTROL_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Tempeh OS live control</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root {
  color-scheme: light dark;
  --bg: #f8f5ee;
  --fg: #211d17;
  --muted: #6b6258;
  --card: #fffaf1;
  --line: #d8ccba;
  --hot: #c6632b;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #151311;
    --fg: #eee7dc;
    --muted: #aaa096;
    --card: #201d19;
    --line: #3a332b;
    --hot: #f6a15f;
  }
}
* {
  box-sizing: border-box;
}
body {
  margin: 0;
  font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  background: var(--bg);
  color: var(--fg);
}
main {
  max-width: 980px;
  margin: 0 auto;
  padding: 28px 18px 48px;
}
h1 {
  margin: 0 0 6px;
  font-size: 2rem;
}
.sub {
  margin: 0 0 22px;
  color: var(--muted);
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 12px;
  margin-bottom: 18px;
}
.card {
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 16px;
  padding: 14px;
}
.label {
  color: var(--muted);
  font-size: 0.85rem;
}
.value {
  margin-top: 6px;
  font-size: 1.45rem;
  font-weight: 750;
  font-variant-numeric: tabular-nums;
}
.chart {
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 20px;
  padding: 12px;
  overflow-x: auto;
}
svg {
  width: 100%;
  min-width: 720px;
  height: auto;
}
.muted {
  color: var(--muted);
}
code {
  background: color-mix(in srgb, var(--card) 70%, var(--line));
  padding: 2px 5px;
  border-radius: 6px;
}
</style>
</head>
<body>
<main>
  <h1>Tempeh OS live control</h1>
  <p class="sub">Live box-air temperature and heater state from the supervised host control loop.</p>

  <section class="cards">
    <div class="card">
      <div class="label">Box air</div>
      <div class="value" id="latest-temp">—</div>
    </div>
    <div class="card">
      <div class="label">Product</div>
      <div class="value" id="product-temp">—</div>
    </div>
    <div class="card">
      <div class="label">Room air</div>
      <div class="value" id="room-temp">—</div>
    </div>
    <div class="card">
      <div class="label">Heater</div>
      <div class="value" id="heater">—</div>
    </div>
    <div class="card">
      <div class="label">Elapsed</div>
      <div class="value" id="elapsed">—</div>
    </div>
    <div class="card">
      <div class="label">Samples</div>
      <div class="value" id="sample-count">0</div>
    </div>
    <div class="card">
      <div class="label">Stream</div>
      <div class="value" id="stream-state">connecting</div>
    </div>
  </section>

  <section class="chart">
    <svg id="chart" viewBox="0 0 900 420" role="img" aria-label="Box temperature and heater state over time">
      <text x="64" y="28" font-size="20" font-weight="700" fill="currentColor">Box temperature + heater</text>
      <text x="18" y="210" font-size="13" fill="currentColor" transform="rotate(-90 18 210)">Temperature °C</text>
      <g id="plot"></g>
    </svg>
  </section>

  <p class="muted">
    CSV: <code id="csv-path">loading…</code>
  </p>
</main>

<script>
const samples = [];
let lastSeq = 0;
let source = null;

const ids = {
  roomTemp: document.getElementById("room-temp"),
  latestTemp: document.getElementById("latest-temp"),
  productTemp: document.getElementById("product-temp"),
  heater: document.getElementById("heater"),
  elapsed: document.getElementById("elapsed"),
  sampleCount: document.getElementById("sample-count"),
  streamState: document.getElementById("stream-state"),
  csvPath: document.getElementById("csv-path"),
  plot: document.getElementById("plot"),
};

function fmtElapsed(seconds) {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return `${m}m ${String(s).padStart(2, "0")}s`;
}

function appendSample(sample) {
  if (sample.seq <= lastSeq) return;
  samples.push(sample);
  lastSeq = Math.max(lastSeq, sample.seq);
}

async function updateStatus() {
  const statusResponse = await fetch("/api/status", { cache: "no-store" });
  if (!statusResponse.ok) return;

  const status = await statusResponse.json();
  ids.csvPath.textContent = status.csv_path;

  if (status.first_retained_seq !== null && lastSeq > 0 && lastSeq < status.first_retained_seq - 1) {
    console.warn("Browser fell behind retained live history; CSV remains the durable full log.");
  }
}

async function catchUp(after = lastSeq) {
  const response = await fetch(`/api/samples?after=${after}`, { cache: "no-store" });
  if (!response.ok) throw new Error(`sample request failed: ${response.status}`);

  const incoming = await response.json();
  for (const sample of incoming) {
    appendSample(sample);
  }

  await updateStatus();
  render();
}

function connectEvents() {
  if (source !== null) {
    source.close();
  }

  source = new EventSource("/events");

  source.onopen = () => {
    ids.streamState.textContent = "live";
    catchUp(lastSeq).catch(error => console.error(error));
  };

  source.addEventListener("sample", event => {
    appendSample(JSON.parse(event.data));
    render();
  });

  source.addEventListener("resync", () => {
    ids.streamState.textContent = "resync";
    catchUp(lastSeq).catch(error => console.error(error));
  });

  source.onerror = () => {
    ids.streamState.textContent = "reconnecting";
    // EventSource reconnects automatically. On open, catchUp(lastSeq) fills
    // anything that arrived while the browser was disconnected.
  };
}

function render() {
  ids.sampleCount.textContent = String(samples.length);

  const latest = samples[samples.length - 1];
  if (!latest) {
    ids.plot.innerHTML = `<text x="64" y="210" fill="currentColor" opacity="0.6">Waiting for temperature readings…</text>`;
    return;
  }

  ids.latestTemp.textContent = `${latest.box_air_temp_c.toFixed(2)} °C`;
  ids.productTemp.textContent =
    latest.product_temp_c === null || latest.product_temp_c === undefined
      ? "—" : `${latest.product_temp_c.toFixed(2)} °C`;
  ids.roomTemp.textContent =
    latest.room_air_temp_c === null || latest.room_air_temp_c === undefined
      ? "—" : `${latest.room_air_temp_c.toFixed(2)} °C`;
  ids.heater.textContent = latest.heater_on ? "on" : "off";
  ids.elapsed.textContent = fmtElapsed(latest.time_s);

  const width = 900;
  const height = 420;
  const margin = { left: 64, right: 24, top: 48, bottom: 54 };
  const plotW = width - margin.left - margin.right;
  const plotH = height - margin.top - margin.bottom;

  const visible = samples.slice(-900);
  const tMin = visible[0].time_s;
  const tMax = Math.max(tMin + 1, visible[visible.length - 1].time_s);

  const tempValues = visible.flatMap(sample => {
    const values = [sample.box_air_temp_c];
    if (sample.product_temp_c !== null && sample.product_temp_c !== undefined) {
      values.push(sample.product_temp_c);
    }
    return values;
  });
  let yMin = Math.min(...tempValues);
  let yMax = Math.max(...tempValues);
  if (Math.abs(yMax - yMin) < 0.5) {
    yMin -= 0.5;
    yMax += 0.5;
  }
  const pad = Math.max(0.25, (yMax - yMin) * 0.12);
  yMin -= pad;
  yMax += pad;

  const x = t => margin.left + ((t - tMin) / (tMax - tMin)) * plotW;
  const y = v => margin.top + (1 - ((v - yMin) / (yMax - yMin))) * plotH;

  let grid = "";
  for (let i = 0; i <= 4; i++) {
    const p = i / 4;
    const gx = margin.left + p * plotW;
    const seconds = tMin + p * (tMax - tMin);
    grid += `<line x1="${gx.toFixed(1)}" y1="${margin.top}" x2="${gx.toFixed(1)}" y2="${margin.top + plotH}" stroke="currentColor" opacity="0.08"/>`;
    grid += `<text x="${gx.toFixed(1)}" y="${height - 24}" text-anchor="middle" font-size="12" fill="currentColor">${fmtElapsed(seconds)}</text>`;
  }
  for (let i = 0; i <= 4; i++) {
    const p = i / 4;
    const gy = margin.top + p * plotH;
    const value = yMax - p * (yMax - yMin);
    grid += `<line x1="${margin.left}" y1="${gy.toFixed(1)}" x2="${width - margin.right}" y2="${gy.toFixed(1)}" stroke="currentColor" opacity="0.08"/>`;
    grid += `<text x="${margin.left - 8}" y="${(gy + 4).toFixed(1)}" text-anchor="end" font-size="12" fill="currentColor">${value.toFixed(1)}</text>`;
  }

  let heaterBands = "";
  let bandStart = null;
  for (const sample of visible) {
    if (sample.heater_on && bandStart === null) bandStart = sample.time_s;
    if (!sample.heater_on && bandStart !== null) {
      const x0 = x(bandStart);
      const x1 = x(sample.time_s);
      heaterBands += `<rect x="${x0.toFixed(1)}" y="${margin.top}" width="${Math.max(1, x1 - x0).toFixed(1)}" height="${plotH}" fill="var(--hot)" opacity="0.16"/>`;
      bandStart = null;
    }
  }
  if (bandStart !== null) {
    const x0 = x(bandStart);
    const x1 = x(tMax);
    heaterBands += `<rect x="${x0.toFixed(1)}" y="${margin.top}" width="${Math.max(1, x1 - x0).toFixed(1)}" height="${plotH}" fill="var(--hot)" opacity="0.16"/>`;
  }

  function linePath(points, valueFn) {
    const validPoints = points.filter(sample => {
      const value = valueFn(sample);
      return value !== null && value !== undefined;
    });
    return validPoints.map((sample, index) => {
      const command = index === 0 ? "M" : "L";
      return `${command} ${x(sample.time_s).toFixed(2)} ${y(valueFn(sample)).toFixed(2)}`;
    }).join(" ");
  }

  const boxPath = linePath(visible, sample => sample.box_air_temp_c);
  const productPath = linePath(visible, sample => sample.product_temp_c);

  const latestX = x(latest.time_s);
  const latestY = y(latest.box_air_temp_c);
  const latestProductX = latest.product_temp_c === null || latest.product_temp_c === undefined
    ? null : x(latest.time_s);
  const latestProductY = latest.product_temp_c === null || latest.product_temp_c === undefined
    ? null : y(latest.product_temp_c);
  const latestProductMarker = latestProductX === null || latestProductY === null
    ? ""
    : `<circle cx="${latestProductX.toFixed(1)}" cy="${latestProductY.toFixed(1)}" r="4.5" fill="var(--hot)"/>`;

  ids.plot.innerHTML = `
    ${grid}
    ${heaterBands}
    <line x1="${margin.left}" y1="${margin.top + plotH}" x2="${width - margin.right}" y2="${margin.top + plotH}" stroke="currentColor" opacity="0.35"/>
    <path d="${boxPath}" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linejoin="round" stroke-linecap="round"/>
    <path d="${productPath}" fill="none" stroke="var(--hot)" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>
    <circle cx="${latestX.toFixed(1)}" cy="${latestY.toFixed(1)}" r="4.5" fill="currentColor"/>
    ${latestProductMarker}
    <rect x="${margin.left}" y="${height - 48}" width="14" height="14" rx="4" fill="var(--hot)" opacity="0.35"/>
    <text x="${margin.left + 22}" y="${height - 36}" font-size="13" fill="currentColor">heater on</text>
    <line x1="${margin.left + 118}" y1="${height - 41}" x2="${margin.left + 148}" y2="${height - 41}" stroke="currentColor" stroke-width="2.6"/>
    <text x="${margin.left + 154}" y="${height - 36}" font-size="13" fill="currentColor">box air</text>
    <line x1="${margin.left + 230}" y1="${height - 41}" x2="${margin.left + 260}" y2="${height - 41}" stroke="var(--hot)" stroke-width="2.4"/>
    <text x="${margin.left + 266}" y="${height - 36}" font-size="13" fill="currentColor">product</text>
  `;
}

catchUp(0).catch(error => console.error(error));
connectEvents();
setInterval(() => {
  updateStatus().catch(error => console.error(error));
}, 5000);
</script>
</body>
</html>
"#;

fn list_serial_ports(show_all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ports = serialport::available_ports()
        .map_err(|error| std::io::Error::other(format!("failed to list serial ports: {error}")))?;

    if ports.is_empty() {
        println!("No serial ports found.");
        return Ok(());
    }

    let candidates = ports
        .iter()
        .filter_map(serial_port_candidate)
        .collect::<Vec<_>>();

    if let Some(best) = best_serial_candidate(&candidates) {
        println!("recommended ESP32 port:");
        println!("  {}", best.port_name);
        println!("    {}", best.reason);
        if best.is_macos_cu {
            println!("    macOS call-out device; preferred for apps opening the port");
        }
        println!("    use:");
        println!(
            "      cargo run -p tempeh-host -- thermometer-test {}",
            best.port_name
        );
        println!();
    } else {
        println!("No obvious ESP32 serial port found.");
        println!(
            "Try unplugging/replugging the ESP32 and running `cargo run -p tempeh-host -- ports --all`."
        );
        println!();
    }

    let other_candidates = candidates
        .iter()
        .filter(|candidate| {
            best_serial_candidate(&candidates)
                .map(|best| best.port_name != candidate.port_name)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    if !other_candidates.is_empty() {
        println!("other likely ESP32-related ports:");
        for candidate in other_candidates {
            println!("  {}", candidate.port_name);
            println!("    {}", candidate.reason);
            if candidate.is_macos_tty {
                println!("    macOS tty variant; usually prefer the matching /dev/cu.* port");
            }
        }
        println!();
    }

    if show_all {
        println!("all serial ports:");
        for port in &ports {
            print_serial_port(port);
        }
    } else {
        println!(
            "Tip: run `cargo run -p tempeh-host -- ports --all` to show Bluetooth/debug/other serial ports too."
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct SerialPortCandidate {
    port_name: String,
    reason: &'static str,
    score: i32,
    is_macos_cu: bool,
    is_macos_tty: bool,
}

fn serial_port_candidate(port: &serialport::SerialPortInfo) -> Option<SerialPortCandidate> {
    let serialport::SerialPortType::UsbPort(info) = &port.port_type else {
        return None;
    };

    let reason = likely_esp32_reason(info)?;
    let is_macos_cu = port.port_name.starts_with("/dev/cu.");
    let is_macos_tty = port.port_name.starts_with("/dev/tty.");

    let mut score = likely_esp32_score(info);

    // On macOS the /dev/cu.* call-out device is usually the right one for
    // applications that initiate the connection. The matching /dev/tty.* entry
    // is the call-in variant and is more likely to block or surprise users.
    if is_macos_cu {
        score += 100;
    }

    if is_macos_tty {
        score -= 100;
    }

    Some(SerialPortCandidate {
        port_name: port.port_name.clone(),
        reason,
        score,
        is_macos_cu,
        is_macos_tty,
    })
}

fn best_serial_candidate(candidates: &[SerialPortCandidate]) -> Option<&SerialPortCandidate> {
    candidates.iter().max_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| b.port_name.cmp(&a.port_name))
    })
}

fn print_serial_port(port: &serialport::SerialPortInfo) {
    println!("  {}", port.port_name);

    match &port.port_type {
        serialport::SerialPortType::UsbPort(info) => {
            println!("    type: USB");
            println!("    vid: {:04X}", info.vid);
            println!("    pid: {:04X}", info.pid);

            if let Some(serial_number) = info.serial_number.as_deref() {
                println!("    serial: {serial_number}");
            }

            if let Some(manufacturer) = info.manufacturer.as_deref() {
                println!("    manufacturer: {manufacturer}");
            }

            if let Some(product) = info.product.as_deref() {
                println!("    product: {product}");
            }

            if let Some(reason) = likely_esp32_reason(info) {
                println!("    likely: {reason}");
            }
        }
        serialport::SerialPortType::BluetoothPort => {
            println!("    type: Bluetooth");
        }
        serialport::SerialPortType::PciPort => {
            println!("    type: PCI");
        }
        serialport::SerialPortType::Unknown => {
            println!("    type: unknown");
        }
    }
}

fn likely_esp32_reason(info: &serialport::UsbPortInfo) -> Option<&'static str> {
    if info.vid == 0x303A {
        return Some("Espressif native USB device");
    }

    if info.vid == 0x10C4 {
        return Some("Silicon Labs CP210x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if info.vid == 0x1A86 {
        return Some("WCH CH340/CH910x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if info.vid == 0x0403 {
        return Some("FTDI USB-UART bridge, sometimes used on ESP32 dev boards");
    }

    let manufacturer = info.manufacturer.as_deref().unwrap_or_default();
    let product = info.product.as_deref().unwrap_or_default();

    if contains_case_insensitive(manufacturer, "espressif")
        || contains_case_insensitive(product, "espressif")
        || contains_case_insensitive(product, "esp32")
    {
        return Some("USB metadata mentions Espressif/ESP32");
    }

    if contains_case_insensitive(manufacturer, "silicon labs")
        || contains_case_insensitive(product, "cp210")
    {
        return Some("Silicon Labs CP210x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if contains_case_insensitive(manufacturer, "wch")
        || contains_case_insensitive(product, "ch340")
        || contains_case_insensitive(product, "ch910")
    {
        return Some("WCH CH340/CH910x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if contains_case_insensitive(manufacturer, "ftdi")
        || contains_case_insensitive(product, "ft232")
    {
        return Some("FTDI USB-UART bridge, sometimes used on ESP32 dev boards");
    }

    None
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn likely_esp32_score(info: &serialport::UsbPortInfo) -> i32 {
    if info.vid == 0x303A {
        return 1_000;
    }

    if info.vid == 0x10C4 || info.vid == 0x1A86 || info.vid == 0x0403 {
        return 700;
    }

    let manufacturer = info.manufacturer.as_deref().unwrap_or_default();
    let product = info.product.as_deref().unwrap_or_default();

    if contains_case_insensitive(manufacturer, "espressif")
        || contains_case_insensitive(product, "espressif")
        || contains_case_insensitive(product, "esp32")
    {
        return 900;
    }

    if contains_case_insensitive(manufacturer, "silicon labs")
        || contains_case_insensitive(product, "cp210")
    {
        return 650;
    }

    if contains_case_insensitive(manufacturer, "wch")
        || contains_case_insensitive(product, "ch340")
        || contains_case_insensitive(product, "ch910")
    {
        return 650;
    }

    if contains_case_insensitive(manufacturer, "ftdi")
        || contains_case_insensitive(product, "ft232")
    {
        return 600;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasmota_heater_normalises_base_url() {
        let heater = TasmotaHeater::new("192.168.1.50/");
        assert_eq!(heater.base_url(), "http://192.168.1.50");
    }

    #[test]
    fn tasmota_heater_preserves_explicit_scheme() {
        let heater = TasmotaHeater::new("https://plug.local/");
        assert_eq!(heater.base_url(), "https://plug.local");
    }
}
