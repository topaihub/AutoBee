@echo off
rem Shim so bare `pnpm` resolves to this project's vendored standalone pnpm
rem (tools/pnpm-standalone/package/bin/pnpm.mjs) instead of a global pnpm whose
rem version may mismatch package.json's `packageManager` and hang on download.
node "%~dp0pnpm.mjs" %*
