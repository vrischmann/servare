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

## Workflow

Run `just check | bunyan` and hack away.

# License

Servare is distributed under [AGPL-3.0-only](/LICENSE)
