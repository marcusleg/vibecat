# CLAUDE.md

## Git rules

- Always create a branch and a draft PR for changes.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages:
  - `feat`: a new feature
  - `fix`: a bug fix
  - `docs`: documentation-only changes
  - `style`: formatting, whitespace, etc. (no code change)
  - `refactor`: code change that neither fixes a bug nor adds a feature
  - `perf`: performance improvement
  - `test`: adding or updating tests
  - `chore`: build process, CI, tooling, or auxiliary changes
  - `ci`: CI configuration and scripts
- Each commit should be a meaningful, self-contained unit of change — something worth a line in a CHANGELOG. If the commit message would be irrelevant or uninteresting in a CHANGELOG, the commit is too small and should be folded into a larger one.
- Mark the PR as ready for review once the work is complete.
- Use rebase merges for PRs. Merge commits are forbidden. Only squash for good reason.
