---
doc_kind: conventions
---

# Agent doc conventions

## ADR format

```yaml
---
adr_id: NNNN
title: <short title>
status: proposed | accepted | superseded | deprecated
date: YYYY-MM-DD
supersedes: [adr_id, ...]
superseded_by: []
---
```

Sections (in order):

1. **Context** — what problem, what's known
2. **Options considered** — at least 2, structurally distinct
3. **Decision** — picked option + binding rule
4. **Consequences** — what this enables, what it forecloses
5. **Cross-references** — other ADRs, findings, source files

## Finding format

```yaml
---
doc_kind: finding
finding_id: <slug>
last_verified_commit: <sha>
dependencies: [adr:NNNN, finding:slug, ...]
---
```

Sections:

1. **Hypothesis** — what we tried to prove
2. **Method** — what we did
3. **Result** — what happened
4. **Conclusion** — actionable takeaway
5. **Cross-references**

## Module format

```yaml
---
doc_kind: module
module_id: <crate-name>
last_verified_commit: <sha>
dependencies: [adr:NNNN, ...]
---
```

Sections:

1. **Purpose** — one-paragraph what
2. **Public surface** — types + functions + invariants
3. **Internal architecture** — sketch
4. **Tests** — what's covered, what's not
5. **Cross-references**

## Commit tag format

Conventional commits + Tx tag:

```
<type>(<scope>): A<wave>.<tx> <description>
```

Examples:

- `feat(server): A1.4 wire SSE dispatch route (Wave A1)`
- `fix(store): A1.2 ADR frontmatter strict validator (Wave A1)`
- `docs(adr): A0.6 land ADR-0005 runner choice (Wave A0)`
