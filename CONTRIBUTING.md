# Contributing

Altair Vega is pre-release. Keep changes focused, reviewable, and release-safe.

## Workflow

1. Fork or branch from the current mainline.
2. Make the smallest change that solves the issue.
3. Run the relevant validation commands from `DEVELOPMENT.md`.
4. Open a pull request with the problem, solution, and validation results.

## Scope

Product functionality is frozen during release hardening. Until release hardening completes, code changes should be limited to:

- Bug fixes found during release validation.
- Test or validation shorthand.
- Packaging and release automation.
- Documentation needed to ship or validate the frozen surface.

Avoid new commands, new transfer modes, new sync modes, or UX feature expansion unless the release scope is explicitly reopened.

## Pull Request Checklist

- Describe the user-visible risk or release-hardening gap being addressed.
- List validation commands run and their results.
- Note any platform or browser matrix coverage that could not be completed.
- Confirm no secrets, production `.env` files, private keys, or release credentials are included.
- Confirm the change does not expand product functionality during release hardening.

## Commit Style

Use a conventional commit subject, such as:

```text
fix: correct browser release build dependencies
```

Keep commit subjects concise and single-line.

## Security

Do not commit secrets, production `.env` files, private keys, or release credentials. Report security-sensitive issues privately instead of opening a public issue.
