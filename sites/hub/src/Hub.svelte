<script lang="ts">
  import { onMount } from 'svelte';
  import EckyMascot from '../../landing/src/EckyMascot.svelte';
  type Component = { componentId:string; displayName:string; params?:{key:string;unit?:string}[]; ports?:{portId:string;typeId:string}[] };
  type Release = {packageId:string;version:string;displayName:string;archiveDigest:string;packageDigest:string;distribution?:{license?:string;repository?:string};header:{tags?:string[];components?:Component[]}};
  type Source = {source:string|null;archiveDigest:string;license?:string;notice?:string};
  let query=$state('');
  let releases=$state<Release[]>([]);
  let pending=$state(false);
  let error=$state<string|null>(null);
  let selected=$state<{release:Release;component:Component}|null>(null);
  let source=$state<Source|null>(null);
  let sourcePending=$state(false);
  let sourceError=$state<string|null>(null);
  let copied=$state(false);
  let searchSerial=0;
  let sourceSerial=0;
  const registry=window.location.origin;
  const installScript=`curl -fsSL ${registry}/install.sh | sh`;
  const command=$derived(selected ? `cairn add ${selected.release.packageId}@${selected.release.version}` : 'cairn search fasteners');
  async function readJson<T>(url:string):Promise<T> {
    const response=await fetch(url,{credentials:'omit'});
    const value=await response.json().catch(()=>({error:`Registry HTTP ${response.status}`}));
    if (!response.ok) throw new Error(typeof value.error==='string' ? value.error : `Registry HTTP ${response.status}`);
    return value as T;
  }
  async function search() {
    const serial=++searchSerial;
    pending=true;error=null;
    try {
      const result=await readJson<{releases:Release[]}>(`/v1/search?${new URLSearchParams({q:query,limit:'100'})}`);
      if(serial===searchSerial) releases=result.releases;
    } catch(failure) {
      if(serial===searchSerial) error=failure instanceof Error?failure.message:String(failure);
    } finally {if(serial===searchSerial)pending=false;}
  }
  async function inspect(release:Release,component:Component) {
    const serial=++sourceSerial;
    selected={release,component};source=null;sourceError=null;sourcePending=true;copied=false;
    try {
      const result=await readJson<Source>(`/v1/packages/${encodeURIComponent(release.packageId)}/${encodeURIComponent(release.version)}/components/${encodeURIComponent(component.componentId)}`);
      if(serial===sourceSerial) source=result;
    } catch(failure) {if(serial===sourceSerial) sourceError=failure instanceof Error?failure.message:String(failure);}
    finally {if(serial===sourceSerial)sourcePending=false;}
  }
  async function copyCommand() {
    try {await navigator.clipboard.writeText(command);copied=true;} catch {copied=false;}
  }
  onMount(()=>{void search();});
</script>

<nav class="nav">
  <div class="nav-inner">
    <a class="brand" href="https://ecky-cad.com/"><span class="brand-mark">E</span><span class="brand-name">Ecky&nbsp;Hub</span></a>
    <div class="nav-links"><a href="#catalog">Components</a><a href="#install">Install CLI</a><a href="https://ecky-cad.com/docs/">Learn ↗</a></div>
  </div>
</nav>
<header class="hero hub-hero">
  <div class="hero-intro">
    <div class="hero-copy">
      <span class="kicker">CAIRN · THE ECKY COMPONENT HUB</span>
      <h1 class="hero-title">Reusable stones.<br />Real components.</h1>
      <p class="hero-lede">Find a useful part. Inspect its parameters, interfaces and source. Install an exact package, or adapt the logic for your next design.</p>
      <div class="hero-cta"><a class="btn btn-primary" href="#catalog">Explore components</a><a class="btn" href="#install">Get the CLI</a></div>
    </div>
    <div class="hero-mascot"><EckyMascot size={190} /></div>
  </div>
</header>
<main class="section hub-catalog" id="catalog">
  <div class="section-head"><span class="kicker">PACKAGES + SOURCE</span><h2>Start with a component.</h2></div>
  <form class="hub-search" onsubmit={(event)=>{event.preventDefault();void search();}}>
    <input type="search" aria-label="Search components and packages" placeholder="Search components, parameters, tags…" bind:value={query} maxlength={512} />
    <button class="btn btn-primary" disabled={pending}>{pending?'SEARCHING…':'SEARCH'}</button>
  </form>
  <div class="hub-status" aria-live="polite">
    {#if error}<span class="hub-error">{error}</span><button class="btn" onclick={()=>void search()}>RETRY</button>
    {:else if pending}<span>Finding reusable components…</span>
    {:else}<span>{releases.length} {releases.length===1?'package':'packages'} · inspect source before reuse</span>{/if}
  </div>
  <div class="hub-browser">
    <section class="hub-results" aria-label="Packages">
      {#if !pending && !error && releases.length===0}<div class="hub-empty">No matching packages. Try a component name or parameter.</div>{/if}
      {#each releases as release (`${release.packageId}@${release.version}`)}
        <article class="hub-package">
          <div class="hub-package-heading"><h3>{release.displayName}</h3><span>{release.version}</span></div>
          <p class="hub-coordinate">{release.packageId}{release.distribution?.license?` · ${release.distribution.license}`:''}</p>
          <div class="hub-tags">{#each release.header.tags ?? [] as tag (tag)}<span>{tag}</span>{/each}</div>
          {#each release.header.components ?? [] as component (component.componentId)}
            <div class="hub-export">
              <div><strong>{component.displayName}</strong><p>{component.params?.map(param=>param.unit?`${param.key} (${param.unit})`:param.key).join(' · ') || 'No parameters'}</p></div>
              <button class="btn" aria-label={`INSPECT ${component.displayName}`} onclick={()=>void inspect(release,component)}>INSPECT</button>
            </div>
          {/each}
          {#if !release.header.components?.length}<p>Component collection · install its pinned dependencies with Cairn.</p>{/if}
        </article>
      {/each}
    </section>
    <aside class="hub-inspector" aria-label="Component source">
      {#if selected}
        <div class="hub-inspector-title"><span class="kicker">SOURCE INSPECTOR</span><h3>{selected.component.displayName}</h3><p>{selected.release.packageId}@{selected.release.version}</p></div>
        <div class="hub-contract"><strong>PARAMETERS</strong><p>{selected.component.params?.map(param=>param.key).join(' · ') || 'None'}</p><strong>PORTS</strong><p>{selected.component.ports?.map(port=>`${port.portId}: ${port.typeId}`).join(' · ') || 'None declared'}</p></div>
        <div class="hub-source-window">
          {#if sourcePending}<p role="status">Loading exact source…</p>
          {:else if sourceError}<p class="hub-error">{sourceError}</p><button class="btn" onclick={()=>selected && void inspect(selected.release,selected.component)}>RETRY SOURCE</button>
          {:else if source?.source}<pre><code>{source.source}</code></pre>
          {:else}<p>{source?.notice || 'Binary geometry export. Install the package to use it in Ecky.'}</p>{/if}
        </div>
        <div class="hub-use"><span class="kicker">USE THIS PACKAGE</span><div class="hub-command"><code>{command}</code><button class="btn" onclick={()=>void copyCommand()}>{copied?'COPIED':'COPY'}</button></div><a class="btn btn-primary" href={`/v1/blobs/${encodeURIComponent(selected.release.archiveDigest)}`} download={`${selected.release.packageId}-${selected.release.version}.ecky-package`}>DOWNLOAD PACKAGE</a><p class="hub-attribution">{source?.license || selected.release.distribution?.license || 'No license declared'} · preserve source attribution when adapting.</p></div>
      {:else}
        <div class="hub-inspector-title"><span class="kicker">LOOK INSIDE</span><h3>Source stays inspectable.</h3><p>Select a component to inspect its contract, read the source and get an exact installation command.</p></div>
        <div class="hub-source-window hub-placeholder"><pre><code>(import-component "package.id"
  :version "1.0.0"
  :component "part"
  :as reusable-part)</code></pre></div>
      {/if}
    </aside>
  </div>
</main>
<section class="section readme-section" id="install"><div class="readme-row"><h2>One command to get started.</h2><div class="readme-prose"><p>Install the Ecky CLI and Cairn client for macOS or Linux. The installer checks the download before switching versions.</p><pre class="hub-install"><code>{installScript}</code></pre><p><code>cairn search</code> finds packages. <code>cairn add package@version</code> installs a pinned collection. <code>cairn install --frozen --offline</code> reuses its verified cache.</p><p>Ecky’s Library → Hub uses this same registry. Agents search and inspect components during design, then verify the resulting model.</p><a class="text-link" href="https://github.com/bobishh/ecky">Source and package documentation ↗</a></div></div></section>
<footer class="footer"><div class="footer-inner"><span>Ecky Hub · Cairn</span><span class="footer-dim">Parametric source. Exact packages.</span><a href="https://ecky-cad.com/">ecky-cad.com ↗</a></div></footer>
