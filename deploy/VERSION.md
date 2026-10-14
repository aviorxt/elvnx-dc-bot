# Version and build information

`/version` returns a private, matching monochrome embed with the installed Cargo package version, numeric build identifier, local-time Discord build timestamp, recent updates/fixes, and **Developed and maintained by avrxt.dev** credit. It accepts no parameters and is available within ELVNX-PVT.

Version comes from `Cargo.toml`. The build identifier is the Unix timestamp captured by `build.rs` when changed source is compiled; it identifies that compiled source build and remains fixed at runtime. Build time uses Discord's timestamp formatting so viewers see their local timezone. A `SOURCE_DATE_EPOCH` value can provide a reproducible build timestamp. Cargo rebuilds this metadata when source or that environment setting changes.

Update the version in Cargo and the recent-updates text when preparing future releases. `elvnx-bot --version` reports the same version/build/developer information without initializing SQLite or connecting to Discord. Credentials, hostnames, SSH paths, and environment secrets are not exposed by the command.
