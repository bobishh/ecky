## Projects as Folders: Edit Anywhere, Stay Canonical

So far every model has lived inside a thread. That is the system of record, but it is not always where you want to type. Sometimes you want to open the source in your own editor, or hand it to an LLM file skill that only knows how to read and write files. A **project folder** is that door: Ecky mirrors one thread's active version onto disk, you edit the plain file, and Ecky picks the change back up — without ever giving up the thread as the canonical history.

`project_folder_export` writes two files:

```text
<projectsRoot>/<slug>/
  model.ecky          edit this with anything
  ecky-project.json   binding manifest, owned by Ecky — never edit by hand
```

Edit `model.ecky` in any editor. A polling watcher in the app notices the file no longer matches the manifest digest and applies it for you: it compiles the source, renders a preview, and commits a new version (named `folder-sync`) on the bound thread. Two safety details make this trustworthy rather than scary:

- **Two-tick settle.** A changed file must read identical on two consecutive polls before the compiler sees it. A half-written save — the editor flushing in chunks — never reaches Ecky mid-write.
- **A broken save fails once, loudly, then waits.** If the edited source does not compile, the watcher reports the failure once for that exact content and then goes quiet until you change the file again. It does not re-render the same mistake every tick.

When you need to reason about the folder explicitly, `project_folder_status` classifies it:

- `clean` — file matches the bound version; nothing to do.
- `fileChanged` — you edited the file; the watcher will apply it (or you can).
- `threadAdvanced` — the thread moved on without the folder; the folder is stale. Re-export to refresh it.
- `conflict` — both sides moved. The watcher will **not** auto-resolve this; applying requires an explicit force, and the previous head stays available as a version so nothing is lost.
- `missing` — no folder or no manifest yet.

The one rule that holds all of this together: **the folder is a mirror, not a second database.** Threads and versions remain the record. A stale folder never silently clobbers the thread, and `ecky-project.json` is Ecky's to write, not yours. Treat the folder as a convenient editing surface and the thread as the truth, and the two stay in sync on their own.
