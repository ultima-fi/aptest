# Aptest
## Installation
#### Source
```
git clone https://github.com/ultima-fi/aptos-tools.git
cd aptos-tools/aptest
cargo build --release
```
#### Cargo & Binary
To come...

## Usage
#### Main
```
A small framework to assist in testing aptos programs

USAGE:
    aptest <SUBCOMMAND>

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information

SUBCOMMANDS:
    help    Print this message or the help of the given subcommand(s)
    init    Initialize a new project
    run     Runs the framework in the current directory
```
#### Init
```
Initialize a new project

USAGE:
    aptest init <NAME>

ARGS:
    <NAME>

OPTIONS:
    -h, --help    Print help information
```
#### Run
```
Runs the framework in the current directory

USAGE:
    aptest run [OPTIONS]

OPTIONS:
    -c, --no-compile                   Removes call to "aptos move compile"
    -d, --start-delay <START_DELAY>    Specifies the number of seconds to wait on the validator
                                       spinning up before trying to interact with it [default: 14]
    -f, --no-faucet                    Run just the validator node, without a faucet
    -h, --help                         Print help information
    -i, --interactive                  Starts validator and waits for Ctrl+C so that end to end
                                       tests can be run manually
    -l, --log                          Logs the output of the validator to a file
    -p, --no-publish                   Removes call to "aptos move publish"
```

## Node Delay
Because it takes a few seconds for the local node to spin up, you can specify a delay with the `-d` option. The default is 14 seconds which worked well for my machine but different machines may need more or less time.

## Todo
* better doc info, specifically about what init creates and what aptest expects in terms of typescript testing files
* slight code tidying (design pattern consistency)
