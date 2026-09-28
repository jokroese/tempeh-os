# Tempeh OS Reference Hardware

- Status: engineering prototype
- Reference region: the Netherlands / EU
- Last updated: 9 September 2026

This page distinguishes the known working prototype from the longer-term
DevKitC-1 target. Historical prices and links record what was bought; they are
not a current shopping basket.

> [!WARNING]
> The working SuperMini prototype completed the first-build checks and made
> tempeh. The ESP32-S3-DevKitC-1 has not yet been tested in this project. Check
> the [current status](../validation.md) before substituting equipment.

## Current controller: SuperMini

The recorded no-load check used a **Nologo ESP32-S3 SuperMini**. It is the board
used for current hands-on development and the build guide. Generic boards sold
as “SuperMini” can differ in flash size, USB support, pin labels and LEDs, so
they are not automatically substitutions.

The firmware currently assumes:

- `box_air` DATA on GPIO13;
- `product` DATA on GPIO4;
- optional `room_air` DATA on GPIO6;
- the BOOT button on GPIO0;
- the built-in addressable status LED on GPIO48;
- at least 4 MB flash.

## Longer-term target: ESP32-S3-DevKitC-1

The DevKitC-1 is the future reference target because it is more consistently
identified and widely available. It is not yet a validated replacement. Its
original revision uses GPIO48 for the addressable LED; revision 1.1 uses GPIO38.
Current firmware uses GPIO48, so revision 1.1 would need a firmware change and
the controller smoke check.

Do not substitute a generic “ESP32” board without checking every requirement
and running the controller smoke check.

## Known prototype parts and unspecified parts

Quantities describe the box-air-only autonomous build plus optional probes.
Parts marked “specify” are necessary but have not been recorded precisely enough
to make this a complete shopping list.

| Role | Reference requirement | Qty | Required | Notes |
| --- | --- | ---: | --- | --- |
| Controller | Nologo ESP32-S3 SuperMini used in recorded no-load test | 1 | Yes | Current build path; verify its flash, USB and LED details before use |
| Heater switch | Enclosed EU smart plug supplied with Tasmota; rated for local mains voltage and the heater load | 1 | Yes | Must accept local HTTP commands; do not open a mains plug for this build |
| Box-air probe | Waterproof DS18B20 probe, 3.0–5.5 V, approximately 1 m cable or longer | 1 | Yes | Controls normal heating; connects to GPIO13 |
| Product probe | Waterproof DS18B20 probe matching the box-air probe | 1 | No | Optional independent cutoff; connects to GPIO4 when enabled |
| Room-air probe | Waterproof DS18B20 probe matching the others | 1 | No | Optional ambient telemetry; connects to GPIO6 |
| Probe interfaces | DS18B20 adapter modules with pull-ups, or documented 4.7 kΩ pull-ups | 1–3 | Yes for each enabled probe | Original MICREEN kit included adapter modules; exact substitute must be specified |
| Heater | Plain seedling heat mat, approximately 20–30 W, without its own thermostat | 1 | Yes | Must remain flat and uncovered |
| Enclosure | Transparent polypropylene box, approximately 40–50 L | 1 | Yes | Outer warm-air chamber only; not a food-contact surface |
| Heat spreader | Thin aluminium or stainless-steel tray, or ceramic tile, covering most of the heater footprint | 1 | Yes | Goes inside the box above the externally mounted heat mat |
| Rack | Raised stainless-steel or food-safe rack fitting inside the box | 1 | Yes | Keeps food bags away from the heat spreader |
| Food bags | Food-contact bags suitable above 35 °C, approximately 20 × 20 cm | As needed | Yes | Perforate both sides; maintain a thin bean layer |
| Low-voltage wiring | Breadboard or secure connector system plus suitable jumper wires | 1 set | Yes | Keep every connection outside wet areas |
| Power/data | USB data cable suitable for the reference ESP32 connector | 1 | Yes | Charge-only cables will not work for flashing |
| Probe mounting | Food-safe clips, ties or another repeatable mounting method | As needed | Yes | Prevent probes touching the heater, spreader or box |

## Food and preparation consumables

The incubation hardware is only one part of making tempeh. The selected
[Domingo Club procedure](https://domingoclub.com/docs/fermentation/how-to-make-tempeh)
also calls for legumes, tempeh starter, vinegar, rice flour and ordinary food
preparation equipment.

For the reference perforated-bag arrangement:

- target a **15–20 mm** bean layer;
- place holes approximately **10–15 mm** apart on both sides;
- use the 1.2 L IKEA ISTAD bags, **21 × 19 cm**, if reproducing the original
  prototype.

## Physical stack

From bottom to top:

1. seedling heat mat, outside the box;
2. polypropylene box floor;
3. heat spreader, inside the box;
4. raised rack;
5. perforated food-contact tempeh bags.

Food must not touch the warm-air chamber, heat spreader or heat mat directly.

## Original Spain purchase record

This is the May 2026 purchase snapshot that produced the first physical
prototype. It is retained for provenance. It is not the Netherlands reference
shopping list; some quantities reflect the original two-probe prototype.

| Role | Exact item used | Supplier | Original link | Qty | Recorded total | Historical status |
| --- | --- | --- | --- | ---: | ---: | --- |
| Controller | ESP32 development board | Existing / generic | — | 1 | €5 | Owned |
| Heater switch | Athom EU plug with Tasmota | Athom | [Product page](https://www.athom.tech/blank-1/EU-plug) | 1 | €10 | Owned |
| Temperature probes | MICREEN DS18B20 waterproof probe kit, two-pack | Amazon Spain | [Product page](https://www.amazon.es/-/en/dp/B0D7SCW33J) | 1 pack | €12 | Owned |
| Heater | Seedling heat mat | Amazon Spain | [Product page](https://www.amazon.es/-/en/dp/B08JD1FB5B) | 1 | €15 | Owned |
| Rack | Food/cooling rack | Amazon Spain | [Product page](https://www.amazon.es/-/en/dp/B0DR199784) | 1 | €11 | Owned |
| Enclosure | IKEA SAMLA 45 L box with lid | IKEA Spain | [Product page](https://www.ikea.com/es/en/p/samla-box-with-lid-transparent-s69440761/) | 1 | €10 | Owned |
| Food bags | IKEA ISTAD 1.2 L / 2.5 L pack | IKEA Spain | [Product page](https://www.ikea.com/es/en/p/istad-resealable-bag-patterned-red-pink-80525674/) | 1 pack | €3 | Owned |
| Heat spreader | Thin aluminium sheet or baking tray | Local / undecided | — | 1 | Not recorded | Needed |
| Wiring and connectors | Jumper wires and breadboard or secure connectors | Local / undecided | — | 1 set | Not recorded | Needed |
| USB cable and probe mounting | Data cable, clips, tape or cable ties | Local / undecided | — | As needed | Not recorded | Needed |

The known priced subtotal was **€66** and the expected practical hardware total
was approximately **€75**, excluding food ingredients. These figures should not
be presented as current Netherlands pricing.

## Substitution rule

Check the electrical and food-contact requirements of a substitute. Repeat the
controller smoke check after changing controller hardware, and watch an
empty-box warm-up after changing the heater or enclosure.
