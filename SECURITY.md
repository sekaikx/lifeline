# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub:
[open a private report](https://github.com/sekaikx/lifeline/security/advisories/new)
(Security tab, *Report a vulnerability*). Do not open a public issue.

Include what you found, how to reproduce it, and the version and operating
system you used. I aim to reply within a week. Once a fix is
released, the advisory is published and you are credited unless you would
rather not be.

## Supported versions

Only the latest release receives fixes.

## Scope

Lifeline is a desktop app. It downloads public data (elevation tiles,
OpenStreetMap, weather forecasts, Sentinel-2 imagery), reads scenario files
and, optionally, an ONNX model file chosen by the user. Problems of interest
include:

- crafted scenario files, server responses or model files that crash the
  app, exhaust memory or run code;
- anything that lets web content in the app's window reach the local system
  beyond the app's own commands;
- installers or release artifacts that do not match the source.

The accuracy of plans and flood maps is not a security issue; please open a
normal issue for that.
