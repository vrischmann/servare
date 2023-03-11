# servare

Servare is a feed aggregator website that you can easily self-host.

# Status

**This is in active development and not really usable yet.**

# Requirements

To run Servare you need:
* a Linux host
* a PostgreSQL server

# Running

## Setting up PostgreSQL

_Note_: you must first create the database that will be used by Servare.

Start by copying the `configuration.toml` file to `/etc/servare.toml` and modify it as you wish.

_(note: documentation for the configuration file will come later)_

Then you can run the application with the following command:
```
servare serve
```

# Developing

## Additional requirements

To develop Servare you need [Just](https://github.com/casey/just), a command runner.

Next you need to run the `install-tools` command like this:
```
just install-tools
```

## First run

By default there is no user configured so you can't login. There's a command to add a user though:

```
$ cargo build
$ ./target/debug/servare users setup-admin foo@bar.com
```

## Working on tests

If you're working on unit or integration tests the workflow usually looks like this:
* run `just check` which continuously runs `cargo check` for quick feedback
* once it compiles, run `cargo test`

## Working on the application

If you're working on the application itself or the UI the worklflow usually looks like this:
* run `just dev` which continuously runs `cargo run -- serve`
* reload the webpage

This is not seamless because of the build time but it's usually fine.

# License

Servare is distributed under [AGPL-3.0-only](/LICENSE)
