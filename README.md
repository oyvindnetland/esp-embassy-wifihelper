# esp-embassy-wifi

Simple helper library to make it easier to connect to wifi with esp-embassy.

## How to use

The `wifi_example.rs` in examples folder show a minimal esp32c3 example code. The part that use this library is this:
 
```rs
let wifi = WifiStack::new(
    spawner,
    peripherals.WIFI,
    peripherals.TIMG0,
    peripherals.RNG,
    peripherals.RADIO_CLK,
    SSID.try_into().unwrap(),
    PASSWORD.try_into().unwrap(),
);
```

## Supported devices

This has been tested on esp32c3 only, but should work (with possibly minor changes) to other esp32 devices.
