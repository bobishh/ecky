<script lang="ts">
  import EckyMascot from './EckyMascot.svelte';
  import ModelWorkbench from './showcase/ModelWorkbench.svelte';

  const repoUrl = 'https://github.com/bobishh/ecky';
  const chaptersUrl = '/docs/chapters/';
  const referenceUrl = '/docs/';
  const sourceExample = `(model
  (params
    (number width 60mm :min 20 :max 120)
    (number thickness 4mm :min 2 :max 10))
  (part plate
    (difference
      (box width 30 thickness)
      (translate 0 0 -1
        (cylinder 3 (+ thickness 2))))))`;
</script>

<nav class="nav">
  <div class="nav-inner">
    <a class="brand" href="/">
      <span class="brand-mark">E</span>
      <span class="brand-name">Ecky&nbsp;CAD</span>
    </a>
    <div class="nav-links">
      <a href="#models">Models</a>
      <a href="#learn">Learn</a>
      <a href={repoUrl} target="_blank" rel="noreferrer">GitHub ↗</a>
    </div>
  </div>
</nav>

<header class="hero" id="case-study">
  <div class="hero-intro">
    <div class="hero-copy">
      <span class="kicker">EXPERIMENTAL DESKTOP CAD · V0.0.1</span>
      <h1 class="hero-title">Parametric parts from code.</h1>
      <p class="hero-lede">Ecky is a desktop CAD app for parts you want to 3D print. Write a model in a small Lisp-style language, adjust its dimensions, and export the geometry. There is optional AI assistance for writing and changing the source.</p>
      <p class="hero-summary">I bought a 3D printer and started writing FreeCAD macros. That grew into a language and a desktop app. Ecky is still a personal experiment: build from source; expect bugs and breaking changes.</p>
      <div class="hero-cta">
        <a class="btn btn-primary" href={chaptersUrl}>Read the chapters</a>
        <a class="btn" href="#models">Inspect working models</a>
      </div>
    </div>
    <div class="hero-mascot">
      <EckyMascot size={190} />
    </div>
  </div>
  <div id="models">
    <div class="models-head">
      <span class="kicker">EXAMPLE PROJECTS · SOURCE + ZIP</span>
      <p>Inspect the source, rotate the geometry, or download all parts and source together as a ZIP.</p>
    </div>
    <ModelWorkbench />
  </div>
</header>

<section class="section readme-section" id="source">
  <div class="source-explainer">
    <div class="readme-prose">
      <span class="kicker">THE SOURCE</span>
      <h2>A model is a text file.</h2>
      <p>This is a complete <code>.ecky</code> model: a plate with a 6 mm hole. The box adds material; the cylinder cuts it away.</p>
      <p><code>width</code> and <code>thickness</code> become controls in the app. Change the width and the plate grows around the centered hole. The cutter follows the thickness, so the hole stays open.</p>
      <a class="text-link" href="/docs/primitive-signatures/#box">Read the geometry functions →</a>
    </div>
    <div class="source-example"><div class="source-filename">plate.ecky</div><pre><code>{sourceExample}</code></pre></div>
  </div>
</section>

<section class="section readme-section" id="workflow">
  <div class="readme-row">
    <h2>What happens in the app</h2>
    <div class="readme-prose">
      <p>Edit the source in Ecky or save it from your own editor. The app rebuilds the model and shows the result beside the code. Previous revisions, including failed ones, stay in the project history.</p>
      <p>You can also ask an agent for a change. Use Codex in Ecky, an API provider, or an external agent through MCP. The result is the same editable source. Inspect it, adjust the parameters, and export STL or STEP when the model supports it.</p>
      <p>No AI account is needed for manual modeling. Files and history stay local; a remote provider receives the model context sent with your request.</p>
    </div>
  </div>
</section>

<section class="section readme-section" id="learn">
  <div class="readme-row">
    <h2>Try it</h2>
    <div class="readme-prose">
      <p>Ecky currently runs from source. Setup needs Node.js, Rust, Tauri prerequisites, and the native geometry runtime. The repository has the installation steps.</p>
      <a class="text-link" href={repoUrl + '#running-from-source'} target="_blank" rel="noreferrer">Build instructions ↗</a>
      <div class="reading-links">
        <a href={chaptersUrl}><strong>Start with a bracket</strong><span>Two boxes, one union, then a dimension change. Continue into fits and larger models.</span></a>
        <a href={referenceUrl}><strong>Function reference</strong><span>Look up arguments, return types, selectors, and examples while writing a model.</span></a>
      </div>
      <p class="project-note">The language and app are under development. A successful render does not prove a part will fit or hold a load. Print small fit samples before a full assembly.</p>
    </div>
  </div>
</section>

<footer class="footer">
  <div class="footer-inner">
    <span>Ecky CAD</span>
    <span class="footer-dim">v0.0.1 pre-release · local desktop CAD</span>
    <a href={repoUrl} target="_blank" rel="noreferrer">github.com/bobishh/ecky</a>
  </div>
</footer>
