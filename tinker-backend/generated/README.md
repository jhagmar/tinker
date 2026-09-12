# Generated HTTP helpers

`tinker codegen` writes `tinker-http.js`: JSDoc typedefs and `fetch` helpers for the public and admin HTTP-JSON routes. Session and wait WebSockets are omitted.

From `tinker-backend/`:

```bash
cargo run -p tinker -- codegen generated/tinker-http.js
```

Edit `crates/tinker-protocol` and re-run that command. `package.json` marks the directory as an ES module for Node import tests.
