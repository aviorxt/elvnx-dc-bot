# Version and build information

`/version` returns a private, matching monochrome embed with the Cargo package version, readable UTC build number, full release identifier, local-time Discord build timestamp, recent updates/fixes, and **Developed and maintained by avrxt.dev** credit. It accepts no parameters and is available within ELVNX-PVT.

Version comes from `Cargo.toml` and follows [Semantic Versioning 2.0.0](https://semver.org/): `MAJOR.MINOR.PATCH`. This feature release is `0.2.0`; the leading zero communicates ongoing initial development. Use minor increments for feature releases and patch increments for fixes; establish `1.0.0` when the public command contract is stable.

Full release format: `0.2.0+build.YYYYMMDD.HHMMSS`, for example `0.2.0+build.20261014.170000`. This uses SemVer build metadata, not a prerelease suffix. The build number is `YYYYMMDD.HHMMSS` in UTC, captured at compile time and fixed at runtime. It is human-readable and sortable within a version; SemVer ignores build metadata for version precedence. There is no universal build-number convention beyond the chosen documented format, and independently compiled builds within the same second can share this number. Binary SHA-256 remains the artifact identity in DEPLOYMENT.md.

The raw epoch is retained separately only for Discord's timestamp markup, so viewers see the build time in their local timezone. `SOURCE_DATE_EPOCH` can provide a reproducible timestamp; invalid values or years outside 1970–9999 fail the build explicitly. Cargo rebuilds metadata when source or that environment setting changes. Windows and Linux use the same formatting. The regression test parses the full identifier using the SemVer library and checks that its readable build date maps back to the captured epoch.

Update the version in Cargo/Cargo.lock and recent-updates text when preparing future releases. `elvnx-bot --version` reports the same full release/build/developer information without initializing SQLite or connecting to Discord. Credentials, hostnames, SSH paths, and environment secrets are not exposed by the command.
