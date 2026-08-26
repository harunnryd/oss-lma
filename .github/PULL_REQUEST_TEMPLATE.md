## Summary

<!-- One-paragraph description of what this PR does and why. Reference any
related issues with `#<issue-number>`. Avoid mentioning internal phase names
(P1/P2/...) or planning docs (`docs/superpowers/...`). -->

## Scope

<!-- Bulleted list of concrete changes. Lead with what a reviewer should
understand before reading the diff. Group related files; mention any new
modules, migrations, or env vars. -->

- <change>
- <change>

## Test plan

<!-- The exact commands a reviewer can run to verify the change end-to-end.
Make these copy-paste-able from the worktree root. -->

- [ ] `uv sync --all-packages`
- [ ] `uv run pytest python -v` — expected: <N> passed
- [ ] `uv run ruff check python` — expected: clean
- [ ] Manual smoke: <if applicable>

## Risk and rollback

<!-- What could break? How do we revert if something goes wrong in production?
Mention schema migrations, lock contention, or behavior changes that touch
existing flows. -->

- **Risk**: <description>
- **Mitigation**: <description>

## Notes for reviewer

<!-- Anything that doesn't fit above: deferred items, follow-up tickets,
gotchas, or context the reviewer would otherwise have to dig for. -->

- <note>

🤖 Generated with [Claude Code](https://claude.com/claude-code)