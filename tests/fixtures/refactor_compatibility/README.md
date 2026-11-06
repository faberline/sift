# Refactor compatibility bytes

These files were captured from `5ae45b9079538af8f70a5bfd4f3c0948adff83a6`.
Only the test code was added before capture. Production code was unchanged.
The tests use fixed event ids, acceptance times, query ids and checkpoint ids.

The event, manifest and query job files must decode and encode to the same bytes.
Each projection must restore its snapshot and produce the same bytes.
Each checkpoint must decode and encode to the same frame bytes.
The checkpoint tests also pin refusal messages and writer error sources.
The CLI file includes visible command paths and the three hidden acceptance commands.
The OpenAPI file pins the whole document, including field and schema order.

Do not regenerate these files as part of the DDD refactor.
