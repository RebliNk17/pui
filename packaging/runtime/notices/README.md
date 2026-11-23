# Runtime notice supplements

These notices supplement files missing from upstream npm tarballs; they do not
replace notices already distributed inside the packages. Generated inventories
link each package to its original or supplemented notice. Versions and integrity
are recorded in the npm lock and official archive pins.

Sources:

- `Pi-MIT.txt`: <https://raw.githubusercontent.com/earendil-works/pi/v1.6.0/LICENSE>
  (applies to the `@earendil-works/pi-*` 1.6.0 workspace packages).
- `data-uri-to-buffer-README.md`: upstream commit
  `85cd8c854aefbf1bb636789d80364cfac8ea1583` README, including its MIT notice.
- `proxy-agents-MIT.txt`: MIT/Nathan Rajlich notice retained from the same
  TooTallNate/proxy-agents monorepo's `agent-base` package. The
  `proxy-agent-negotiate` 1.1.0 package declares MIT but omits the text.
- `standardwebhooks-Apache-2.0.txt`: upstream standard-webhooks commit
  `b4d2c14fc5b4ccff3ff271e3b087dff812254c59` LICENSE.
- `Apache-2.0.txt`: Apache 2.0 text retained from AWS SDK's `@aws-sdk/core` package;
  missing AWS notices retain AWS SDK attribution in the mapping manifest.
- `npm-ISC.txt`: npm's ISC notice retained from its `@npmcli/fs` package.
- `eastasianwidth-MIT.txt`: upstream `komagata/eastasianwidth` MIT-LICENSE.txt;
  upstream README explicitly explains the 0.2.0 missing-notice history.
- `err-code-MIT.txt`: MIT terms with IndigoUnited attribution; the 2.0.3
  package.json and upstream README declare MIT but omit the license text.
- `imurmurhash-README.md`: upstream jensyt/imurmurhash-js README with MIT text and
  Gary Court/Jens Taylor attribution.
- `spdx-exceptions-README.md`: upstream `kemitchell/spdx-exceptions.json` v2.5.0
  README's Linux Foundation attribution and licensing explanation.
- `spdx-license-ids-README.md`: upstream jslicense/spdx-license-ids README.
- `CC-BY-3.0.txt` and `CC0-1.0.txt`: Creative Commons legalcode texts, from
  <https://creativecommons.org/licenses/by/3.0/legalcode.txt> and
  <https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt>.

All runtime packages keep their own package.json, source/bundled assets and
original notices. These supplemental notices were obtained during development;
packaging does not fetch mutable license files from the network.
