# Third-party notices

AI Route Manager includes or redistributes the components below. The project
license does not replace the licenses of these components.

## Mihomo

- Project: MetaCubeX/mihomo
- Version: 1.19.30, Windows amd64 build with gVisor
- Source: https://github.com/MetaCubeX/mihomo/tree/v1.19.30
- License: GNU General Public License v3.0 only
- Packaged file: `src-tauri/resources/engine/bin/mihomo.exe`
- SHA-256: `F55B3028D9160BEB9044F21B05DD7405B46524614A19642D6291492F5F985761`
- Modification: redistributed without source changes; AI Route Manager starts it
  as a separate process and generates configuration files for it.

The corresponding upstream source is available at the source URL above. A copy
of GPL-3.0 is provided in [`LICENSE`](LICENSE).

## MetaCubeX routing data

- Project: MetaCubeX/meta-rules-dat
- Source: https://github.com/MetaCubeX/meta-rules-dat
- License: GNU General Public License v3.0 only
- Packaged files:
  - `GEOIP.dat` — `2DEA9374371EE651B15705293EBD25076CC62948EB8FB8121D69E25DB5E1ADD8`
  - `GEOIP.metadb` — `7722AB3C5518A27DE61F4D444528DBDF3D6067D1614B6C84C66B732FBF9021D7`
  - `GEOSITE.dat` — `098B98841C252E4B0C83CFEA9FAE19A97B018E142070406F019F7343B9784AD5`

The data files were packaged without local modification. Their upstream build
combines public routing datasets documented by MetaCubeX; upstream notices and
source links remain applicable.

## GeoLite2 ASN data

- Project: xishang0128/geoip (`GeoLite2-ASN.mmdb` release artifact)
- Source: https://github.com/xishang0128/geoip
- License: Creative Commons Attribution-ShareAlike 4.0 International
- Packaged name: `src-tauri/resources/engine/resources/ASN.mmdb`
- SHA-256: `C0507CFB091F42AC525F4EB8A0051A7897F5FAC72E852799E01096583566033B`
- Attribution: This product includes GeoLite2 data created by MaxMind,
  available from https://www.maxmind.com/.

## JavaScript and Rust dependencies

The application also links dependencies listed in `pnpm-lock.yaml` and
`src-tauri/Cargo.lock`. Their individual license terms remain applicable.
Release builds should retain the lockfiles and generate a dependency license
report as part of the release audit.
