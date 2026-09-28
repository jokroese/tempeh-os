# Prototype Safety Notes

This is a prototype, not an unattended appliance.

- Complete the [validation gates](../validation.md) in order.
- Use the autonomous ESP32 route for normal operation; the laptop monitor is
  read-only.
- Do not put food directly on the SAMLA box.
- Keep mains plug and cable joins outside the humid chamber.
- Do not open, modify or serial-flash mains-powered plugs as part of this build.
- Do not fold or cover the heat mat with insulating material.
- Use a heat spreader between heat source and food packets.
- Start with supervised empty-box tests.
- Keep the implemented 34 °C box-air cutoff. If the optional product probe is
  enabled, keep its 34 °C cutoff unless a reviewed test plan changes it.
- Configure the Tasmota plug to remain off after power loss if supported.
- Do not run a new or materially changed build unattended until its thermal
  behaviour has been characterised.
- Treat loss of temperature telemetry, a red status LED or an unexplained reset
  as a stopped batch that requires investigation.
- Tempeh OS controls incubation temperature; it does not determine whether food
  is safe. Follow the linked food procedure and appropriate food-safety
  guidance.
