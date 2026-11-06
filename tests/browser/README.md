# Browser E2E

```powershell
npm install --prefix tests/browser
cargo build
$env:NEOSKIN_BROWSER_E2E='1'
$env:NEOSKIN_CHROME_PATH='C:\Program Files\Google\Chrome\Application\chrome.exe'
cargo test --test browser -- --nocapture
```

`tests/browser.rs` builds a temporary SQLite DB, seeds `admin@example.com / secret123`,
starts `neoskin_rs serve` on a free port and runs `flows.mjs` against it.
Without `NEOSKIN_BROWSER_E2E=1` the test is skipped.
