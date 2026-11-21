# Security

## Reporting a security problem

Please don't report security problems in a public issue. Report them privately instead: open the [Security tab](https://github.com/TanishkKamra/bootmend/security) and choose **Report a vulnerability**. Only the maintainer can see the report, and the reply comes there too.

It helps to include the Bootmend version (at the bottom of Settings), what you did, what happened, and what someone could do with it.

Once a fix is out, the release notes say what was fixed.

## Supported versions

Only the newest release gets fixes. Bootmend offers new versions itself, so staying current takes one click.

## How updates are protected

- Each release is built and signed by the release workflow on GitHub, not on anyone's PC. The build never sees the signing key: a separate job signs the finished exe, and only version tags can reach the key.
- Bootmend installs an update only when the download carries a valid signature from Bootmend's release key and the signature names the version it asked for. A tampered download, or an older release passed off as new, is refused.
- Updates download over HTTPS only, from this repository's releases.
