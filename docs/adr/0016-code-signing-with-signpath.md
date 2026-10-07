# ADR 0016: code signing with the SignPath Foundation

Status: accepted · 2026-10-07 (chosen by the maintainer)

## Context

The 0.1.0 beta 1 installers are not signed (ADR 0012 §5), so Windows SmartScreen and macOS
Gatekeeper warn on first launch. The options looked at for an open-source project:

| Option | Cost | Notes |
| --- | --- | --- |
| SignPath Foundation | Free | For OSI-licensed projects built in CI; the certificate is the Foundation's. |
| Certum Open Source | About €49 a year | A certificate in the maintainer's name, identity checked. |
| Azure Artifact Signing | Paid | Not offered to individuals in Brazil. |
| Microsoft Store (MSIX) | Free for individuals | The only way with no warning at all; a separate package. |
| Apple Developer Program | US$99 a year | The only way to sign and notarize for macOS. |

An EV certificate no longer skips SmartScreen's reputation check, so no paid certificate
removes the warning on day one.

## Decisions

1. **Windows installers are signed through the SignPath Foundation**, once it accepts cloudrs.
   The maintainer applies; Certum Open Source is the fallback.
2. **Only CI builds are signed.** `.github/workflows/release.yml` uploads the NSIS installer,
   submits it with `signpath/github-action-submit-signing-request@v3` and attaches the signed
   copy to the run as `cloudrs-windows-signed`. Each request is approved by hand on SignPath.
3. **The steps stay off until configured.** They run only when the repository variable
   `SIGNPATH_PROJECT_SLUG` is set, so releases keep working while the application is pending.
   The repository needs:
   - variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`,
     `SIGNPATH_SIGNING_POLICY_SLUG` (usually `release-signing`);
   - secret `SIGNPATH_API_TOKEN` (a SignPath user with submitter rights).
4. **The installer is signed, not the executable inside it.** SignPath does not open NSIS
   installers; signing `cloudrs.exe` before packaging would need a second request per release.
   It can come later.
5. **The README carries the "Code signing policy" section** SignPath requires: the attribution,
   team roles and a privacy statement.
6. **macOS stays unsigned** until there is an Apple Developer account.

## SignPath artifact configuration

The upload step zips the installer, so the project's artifact configuration is:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file path="cloudrs_*_x64-setup.exe">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

## Consequences

- Signed installers name their publisher instead of "Unknown publisher". SmartScreen's warning
  fades as downloads build the certificate's reputation, not at once.
- A release waits for the maintainer's approval on SignPath (up to an hour before the job
  gives up).
