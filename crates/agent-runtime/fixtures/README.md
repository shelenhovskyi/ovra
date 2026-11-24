# Recorded claude sessions

Contract tests in `src/tests.rs` replay these files against the driver. Each
line is one step:

- `{"cli": {...}}`: the CLI writes this line to stdout.
- `{"ovra": {...}}`: Ovra must write exactly this line to stdin next.
- `{"wait": {...}}`: wait for a core event that contains these fields.
- `{"core": {...}}`: a request to the core, as the UI would send it.

After the last step the CLI expects stdin to close with nothing more on it.

The current files were written by hand from the protocol observed on Claude
Code 2.1.331 (`docs/specs/claude-cli-protocol.md`). Task 2.H replaces them
with recordings of real sessions; keep the step format.
