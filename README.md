# servare

Servare is a feed aggregator website that you can easily self-host.

# Status

**This is in active development and not really usable yet.**

# Requirements

To run Servare you need:
* a Linux host
* a PostgreSQL server
* a [Rust](https://rustup.rs) installation to get the `cargo` tool.
* the [sqlx-cli](https://crates.io/crates/sqlx-cli) tool to run migrations.

# Running

## Setting up PostgreSQL

You must run the migrations first, which you can do like this:
```
DATABASE_URL=postgres://vincent:vincent@localhost/servare sqlx database setup
```

## Configuring the application

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

## Workflow

Run `just check | bunyan` and hack away.

# License

Servare is distributed under [AGPL-3.0-only](/LICENSE)
