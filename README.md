# Beep SFU server
Beep SFU is an implementation of a SFU server for the messaging app Beep.
## CLI Documentation : 
```
Usage: beep-sfu [OPTIONS]

Options:
      --dev

      --host <HOST>
          [default: 127.0.0.1]
  -s, --signal-port <SIGNAL_PORT>
          [default: 8080]
      --media-port-min <MEDIA_PORT_MIN>
          [default: 3478]
      --media-port-max <MEDIA_PORT_MAX>
          [default: 3479]
  -e, --env <ENV>
          [default: prod]
  -d, --debug

  -l, --level <LEVEL>
          [default: info] [possible values: error, warn, info, debug, trace] #not working yet
  -h, --help
          Print help
  -V, --version
          Print version

Example for running in production with 10 workers :
    beep-sfu --media-port-min 3478 --media-port-max 3588 --env prod
```
## How to run it ?
### Dev mode
```
cargo run -- -env dev
```
**Dev mode features** :
- Logging in stdout
### Production mode (default)
```
# mkdir /var/log/beep-sfu 
# chown <your-user>:<your-group> /var/log/beep-sfu
$ cargo run
```
**Production mode features** :
- Logging in a dedicated file `/var/log/beep-sfu/beep-sfu.log<timestamp>`
