use std::env;
use std::fs;
use std::path::Path;

use tempeh_control::{ControlReading, Controller, Heater, TasmotaHeater, run_trace_control};
use tempeh_model::EnvironmentState;
use tempeh_sim::{SimConfig, Simulator, TemperatureTrace};

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
        "plug-test" => {
            run_plug_test(env::args().nth(2))?;
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
        "Usage:\n  cargo run                         # write out/sim.html\n  cargo run -- html                 # write out/sim.html\n  cargo run -- csv                  # print simulation CSV\n  cargo run -- control              # print simulated control-loop CSV\n  cargo run -- plug-test <url>      # turn Tasmota plug on, wait, turn off\n\nEnvironment:\n  TEMPEH_TASMOTA_URL=http://192.168.1.50"
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

fn render_html(samples: &[EnvironmentState], config: &SimConfig) -> String {
    let final_state = samples.last().copied().unwrap_or_else(|| samples[0]);

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
</style>
</head>
<body>
<main>
<h1>Tempeh OS simulation v0</h1>
<p>
This is a first toy model of the incubator: room air, box air, tempeh core,
fermentation progress, metabolic heat, and heater state. This report is generated by composing the
controller with the simulated environment. The model deliberately leaves out vent and fan.
Run <code>cargo run -- csv</code> when you want the raw data.
</p>

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
        progress = final_state.fermentation_progress * 100.0,
        temp_svg = temp_svg,
        progress_svg = progress_svg,
        heater_svg = heater_svg,
        rows = rows,
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
        let heater_on = controller.update(state.box_air_temp_c, state.tempeh_core_temp_c);
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
                "provide a Tasmota URL, e.g. cargo run -- plug-test http://192.168.1.50",
            )
        })
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
