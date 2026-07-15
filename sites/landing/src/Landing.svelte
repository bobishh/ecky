<script lang="ts">
  import EckyMascot from './EckyMascot.svelte';

  const repoUrl = 'https://github.com/bobishh/ecky';
  const releasesUrl = 'https://github.com/bobishh/ecky/releases';
  const docsUrl = '/docs/';

  const layers = [
    {
      tag: 'SURFACE',
      title: 'You write Scheme',
      body: 'An .ecky file is parenthesized Scheme: (model (part ...)). Friendly to read and write — and it is not the thing that gets built. It is a skin over the layer below.',
    },
    {
      tag: 'CORE IR',
      title: 'It lowers to a finite IR',
      body: 'A small, fixed set of core operations: primitives, booleans, selectors, placements, repeats. The kernel never sees arbitrary Scheme, only this closed vocabulary.',
    },
    {
      tag: 'BACKEND',
      title: 'It renders on a kernel',
      body: 'Native OCCT by default — exact B-rep, selectable faces and edges. build123d and FreeCAD are follower backends for cross-check and import.',
    },
  ];

  const features = [
    {
      title: 'Version history',
      body: 'Every iteration is persisted in SQLite. Scroll back through the model, fork a design into a new thread.',
    },
    {
      title: 'Eyes on the result',
      body: 'Viewport screenshots are fed back to the LLM between iterations, so it can correct itself from what it sees.',
    },
    {
      title: 'Edit the IR by hand',
      body: 'The generated IR is readable text. Tweak a dimension, add a fillet, commit it. No opaque script to reverse-engineer.',
    },
    {
      title: 'Bring your own model',
      body: 'Gemini, any OpenAI-compatible endpoint, or local Ollama. Pick the provider and model in settings.',
    },
    {
      title: 'Agent-ready (MCP)',
      body: 'A built-in MCP server lets an external coding agent author models with its own tools: inspect, validate, preview, commit.',
    },
    {
      title: 'Verify by invariant',
      body: 'State a requirement in a verify clause and Ecky checks it red-to-green, independent of whatever geometry renders.',
    },
  ];
</script>

<nav class="nav">
  <a class="brand" href="/">
    <span class="brand-mark">E</span>
    <span class="brand-name">Ecky&nbsp;CAD</span>
  </a>
  <div class="nav-links">
    <a href={docsUrl}>Docs</a>
    <a href={repoUrl} target="_blank" rel="noreferrer">GitHub ↗</a>
    <a class="nav-cta" href={releasesUrl} target="_blank" rel="noreferrer">Download</a>
  </div>
</nav>

<header class="hero">
  <div class="hero-mascot">
    <EckyMascot size={220} />
  </div>
  <p class="stamp">v0.0.1 · pre-release</p>
  <h1 class="hero-title">Prompt-driven CAD</h1>
  <p class="hero-lede">
    Describe a part in words. An LLM writes it in a small modeling language.
    It renders as an exact B-rep solid you can read, edit, and version.
  </p>
  <div class="hero-cta">
    <a class="btn btn-primary" href={releasesUrl} target="_blank" rel="noreferrer">Download ↗</a>
    <a class="btn" href={repoUrl} target="_blank" rel="noreferrer">Source ↗</a>
    <a class="btn" href={docsUrl}>Docs</a>
  </div>
</header>

<section class="section">
  <div class="section-head">
    <span class="kicker">HOW IT WORKS</span>
    <h2>Three layers, on purpose</h2>
    <p class="section-sub">The LLM never emits a mesh or a script. It writes <code>.ecky</code>, which compiles through three layers.</p>
  </div>
  <div class="layer-grid">
    {#each layers as layer}
      <article class="layer-card">
        <span class="layer-tag">{layer.tag}</span>
        <h3>{layer.title}</h3>
        <p>{layer.body}</p>
      </article>
    {/each}
  </div>
</section>

<section class="section section-code">
  <div class="section-head">
    <span class="kicker">WHAT IT LOOKS LIKE</span>
    <h2>A parametric enclosure</h2>
    <p class="section-sub">Named dimensions, a hollow body with a vent bored through it, a filleted top edge, and a <code>verify</code> clause that pins the lid clearance.</p>
  </div>
  <pre class="code"><code>{`(model
  (params
    (number body_w 80 :label "Body width")
    (number body_d 50 :label "Body depth")
    (number body_h 20 :label "Body height")
    (number wall    2 :label "Wall"))

  (verify
    (tag lid_clearance body.lid_gap)
    (metric gap (clearance min-distance body lid))
    (expect gap (>= 0.3)))

  (part body
    (build
      (shape hollow (shell wall :faces "top" (box body_w body_d body_h)))
      (shape vent   (translate 0 0 -0.5 (cylinder 3 (+ body_h 1))))
      (result
        (fillet 1.5 :edges "top"
          (difference hollow vent)))))`}</code></pre>
</section>

<section class="section">
  <div class="section-head">
    <span class="kicker">FEATURES</span>
    <h2>What ships today</h2>
  </div>
  <div class="feature-grid">
    {#each features as feature}
      <article class="feature-card">
        <h3>{feature.title}</h3>
        <p>{feature.body}</p>
      </article>
    {/each}
  </div>
</section>

<section class="section section-gallery">
  <div class="section-head">
    <span class="kicker">REAL MODELS</span>
    <h2>Built with Ecky</h2>
    <p class="section-sub">Coming soon: a kid's rubber stamp — a flexible TPU press face on a rigid PLA handle, designed end-to-end in .ecky.</p>
  </div>
  <div class="gallery">
    <div class="gallery-slot gallery-placeholder">
      <span>stamp · TPU + PLA</span>
    </div>
    <div class="gallery-slot gallery-placeholder">
      <span>slot reserved</span>
    </div>
    <div class="gallery-slot gallery-placeholder">
      <span>slot reserved</span>
    </div>
  </div>
</section>

<section class="cta-section">
  <div class="cta-card">
    <h2>Start building</h2>
    <p>Ecky is early and pre-release. Expect rough edges and breaking changes.</p>
    <div class="cta-row">
      <a class="btn btn-primary" href={releasesUrl} target="_blank" rel="noreferrer">Releases ↗</a>
      <a class="btn" href={repoUrl} target="_blank" rel="noreferrer">GitHub ↗</a>
      <a class="btn" href={docsUrl}>Field guide</a>
    </div>
  </div>
</section>

<footer class="footer">
  <div class="footer-inner">
    <span>Ecky CAD</span>
    <span class="footer-dim">Prompt-driven CAD · exact B-rep solids</span>
    <a href={repoUrl} target="_blank" rel="noreferrer">github.com/bobishh/ecky</a>
  </div>
</footer>
