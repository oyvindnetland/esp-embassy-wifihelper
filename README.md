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
    SSID.try_into().unwrap(),
    PASSWORD.try_into().unwrap(),
);
```

## How to set up and start delayed through a channel

For cases where wifi is not supposed to connect at startup, or the ssid/password is unknown at startup, a variant for
connecting is used. The correct hardware resources are set up and the tasks are spawned, but it waits for a message
on a channel with the ssid/password information.

```rs
static CHANNEL: Channel<CriticalSectionRawMutex, ClientConfiguration, 1> = Channel::new();

let wifi = WifiStack::new_connect_later(
    spawner,
    peripherals.WIFI,
    peripherals.TIMG0,
    peripherals.RNG,
    CHANNEL.receiver(),
);
```

The wifi will then try to connect with:
```rs
let client_config = ClientConfiguration {
    ssid: SSID.try_into().unwrap(),
    password: PASSWORD.try_into().unwrap(),
    ..Default::default()
};
let _ = CHANNEL.send(client_config).await;
```

## Supported devices

This has been tested on esp32, esp32c3, esp32s3 and esp32c6, and is assumed to work on the remaining esp32 devices as well.
