# Languages

Each folder is one language, described by data only. The editor
(`ui/src/editor`) reads every `*/language.json` at build time; adding a
language needs no editor code beyond its grammar entry in
`ui/src/editor/grammars.ts`.

`<id>/language.json`, where `<id>` is the folder name:

| Field        | Meaning                                                                 |
| ------------ | ----------------------------------------------------------------------- |
| `name`       | Display name.                                                           |
| `extensions` | File extensions without the dot, matched case-insensitively.            |
| `filenames`  | Optional exact file names, e.g. `.babelrc`; they win over extensions.   |
| `grammar`    | The tree-sitter `.wasm` grammar, as `<npm package>/<file>`.             |
| `highlights` | Highlight queries, as paths relative to this folder. Earlier files win. |

Highlight queries use the usual tree-sitter capture names (`keyword`,
`string`, `function.macro`, ...). The editor turns `a.b` into the CSS classes
`tok-a tok-a-b`, so themes can style a whole group or one sub-capture.
Captures starting with `_` are ignored.

## Adding a language

1. Add the grammar package to `ui/package.json` (pinned version).
2. Import its `.wasm` in `ui/src/editor/grammars.ts`.
3. Add `<id>/language.json` and `<id>/highlights.scm`.
4. `just test` checks that every query compiles against its grammar.

## Sources

The `highlights*.scm` files are copied from the `queries/` folder of each
grammar's npm package (all MIT licensed): `tree-sitter-javascript` 0.25.0,
`tree-sitter-typescript` 0.23.2, `tree-sitter-rust` 0.24.0,
`tree-sitter-python` 0.25.0, `tree-sitter-json` 0.24.8, `tree-sitter-css`
0.25.0.
