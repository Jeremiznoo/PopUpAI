# Contributing

PopUpAI is in a design phase; read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
and the [ADRs](docs/ADR/) before proposing changes to the structure.

## Workflow

- Branch from `main`; keep changes small and reviewable.
- Use conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`,
  `chore:`, `security:`.
- Never rewrite shared history.
- Update the relevant documentation in the same change, and add a line to
  [CHANGELOG.md](CHANGELOG.md).

## Quality bar

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  must pass. Understand a warning before silencing it.
- Add automated tests where a desktop is not required. Desktop behaviour that
  cannot be automated goes into the manual checklist in
  [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) and is never described as an
  automated pass.
- Do not claim a platform feature works until it has been tested there.
- No platform-specific logic in `popupai-core`, `popupai-ai` or
  `popupai-security`.

## Security

- Never commit credentials, tokens or personal machine configuration.
- Never log secrets, prompts or answers by default.
- Add a dependency only when it is clearly needed, and say why in the change.
- Report vulnerabilities as described in [SECURITY.md](SECURITY.md).
