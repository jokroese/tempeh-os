# Tempeh OS Hardware v0 BOM

Status: prototype  
Location: Spain  
Currency: EUR  
Last updated: 2026-05-28

This BOM documents the exact parts used for the first real prototype. It is not yet a final recommended build. Prices and product links are a snapshot.

## Design intent

A low-cost, reproducible tempeh incubator using a plastic storage box, seedling heat mat, two DS18B20 temperature probes, ESP32, and a Tasmota smart plug.

## Parts

| Role               | Exact item used                              | Supplier           | Link                                                                           |     Qty | Unit price | Total | Required? | Status | Substitution spec                                                                                    | Notes                                                             |
| ------------------ | -------------------------------------------- | ------------------ | ------------------------------------------------------------------------------ | ------: | ---------: | ----: | --------- | ------ | ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Microcontroller    | ESP32 dev board                              | generic / existing |                                                                                |       1 |         €5 |    €5 | yes       | owned  | ESP32-class board with Wi-Fi and enough GPIO for a DS18B20 1-Wire bus.                               | Reads temperature probes.                                         |
| Heater switch      | Athom EU plug with Tasmota                   | Athom              | https://www.athom.tech/blank-1/EU-plug                                         |       1 |        €10 |   €10 | yes       | owned  | Tasmota-compatible smart plug or relay rated for the heater load and local mains voltage.            | Switches heat mat.                                                |
| Temperature probes | MICREEN DS18B20 waterproof probe kit, 2-pack | Amazon Spain       | https://www.amazon.es/-/en/dp/B0D7SCW33J                                       |       1 |        €12 |   €12 | yes       | owned  | 2 × waterproof DS18B20 probes, 3.0–5.5 V, 1-Wire, preferably 1 m cable or longer.                    | Two probes: air and product/dummy mass.                           |
| Heater             | Seedling heat mat                            | Amazon Spain       | https://www.amazon.es/-/en/dp/B08JD1FB5B                                       |       1 |        €15 |   €15 | yes       | owned  | Plain seedling heat mat, roughly 20–30 W, no built-in controller preferred.                          | Controlled by Tasmota.                                            |
| Rack               | Food/cooling rack                            | Amazon Spain       | https://www.amazon.es/-/en/dp/B0DR199784                                       |       1 |        €11 |   €11 | yes       | owned  | Stainless or food-safe metal rack with raised feet, fitting inside box with airflow clearance.       | Keeps tempeh packets off heat spreader.                           |
| Enclosure          | IKEA SAMLA 45 L box with lid                 | IKEA Spain         | https://www.ikea.com/es/en/p/samla-box-with-lid-transparent-s69440761/         |       1 |        €10 |   €10 | yes       | owned  | Transparent PP/plastic box, roughly 40–50 L, floor large enough for mat/rack, lid can sit loosely.   | Outer warm-air chamber only; not food-contact.                    |
| Food bags          | IKEA ISTAD resealable bags, 1.2 L / 2.5 L pack | IKEA Spain       | https://www.ikea.com/es/en/p/istad-resealable-bag-patterned-red-pink-80525674/ |  1 pack |         €3 |    €3 | yes       | owned  | Food-contact freezer/zip bags suitable above 35 °C and able to be perforated. Preferred bag size is about 20 × 20 cm. | Use the 1.2 L bags, 21 × 19 cm, for first experiments. The included 2.5 L bags can be used later if the bean layer remains thin. Perforate before incubation. |
| Heat spreader      | Thin aluminium sheet or baking tray          | local / TBD        |                                                                                |       1 |        TBD |   TBD | yes       | needed | Aluminium/stainless tray or ceramic tile covering most of heater footprint.                          | Spreads heat and prevents local hotspots.                         |
| Wiring             | Jumper wires                                 | generic / TBD      |                                                                                |   1 set |        TBD |   TBD | yes       | needed | Jumper wires compatible with ESP32 and DS18B20 adapter/probe wiring.                                 | ESP32 to probes.                                                  |
| Prototyping        | Breadboard                                   | generic / TBD      |                                                                                |       1 |        TBD |   TBD | yes       | needed | Small breadboard, screw terminal, Wago-style connector, or equivalent prototyping connection method. | First wiring prototype.                                           |
| Power/data         | USB data cable for ESP32                     | generic / TBD      |                                                                                |       1 |        TBD |   TBD | yes       | needed | USB data cable, not charge-only.                                                                     | Used for flashing, serial logs, and power during prototype tests. |
| Probe mounting     | Clips, tape, or cable ties                   | generic / TBD      |                                                                                | various |        TBD |   TBD | yes       | needed | Any fastening method that holds probes repeatably without contaminating food.                        | Holds probes in place.                                            |
| Food               | Beans                                        | TBD                |                                                                                | various |        TBD |   TBD | yes       | needed | Soybeans or other beans suitable for tempeh experiments.                                             | Excluded from hardware subtotal.                                  |
| Starter            | Tempeh starter culture                       | TBD                |                                                                                |       1 |        TBD |   TBD | yes       | needed | Rhizopus tempeh starter culture from a food-safe supplier.                                           | Excluded from hardware subtotal.                                  |
| Acid               | Vinegar                                      | TBD                |                                                                                |       1 |        TBD |   TBD | yes       | needed | Food-grade vinegar for the acidification step.                                                       | Excluded from hardware subtotal.                                  |

## Cost summary

Known priced hardware items: **€66**

Unpriced required hardware items:

- heat spreader
- jumper wires
- breadboard or connector method
- USB data cable
- probe mounting materials

Expected practical hardware total: roughly **€75**, excluding beans, starter culture, vinegar, and other food-preparation consumables.

## Notes on substitution

The `Exact item used` column documents this prototype as built. The `Substitution spec` column is the more portable recipe.

When reproducing the build elsewhere, prefer matching the substitution spec over chasing identical product links. Product listings, prices, and availability will change.

## Food-contact boundary

The SAMLA box is used only as the outer warm-air chamber. Food should remain inside perforated food-contact bags or other food-safe containers.

Prototype stack:

1. seedling heat mat
2. SAMLA box
3. heat spreader
4. rack
5. perforated ISTAD tempeh bags

## Tempeh bag size

Use the **1.2 L ISTAD bags** from the 1.2 L / 2.5 L pack for the first experiments.

- 1.2 L bag size: **21 × 19 cm**
- target bean layer: **15–20 mm thick**
- hole spacing: roughly **10–15 mm**, on both sides

The included 2.5 L bags are larger, **25 × 25 cm**, and are not the default for v0 because they encourage larger slabs. They are acceptable later if the bean layer is still kept to 15–20 mm thick.
