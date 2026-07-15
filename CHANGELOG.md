# Changelog

## [0.2.0](https://github.com/bobishh/ecky/compare/v0.1.0...v0.2.0) (2026-07-15)


### Features

* **agent-engine-config:** per-model vision capability memory and single active engine ([ea66f93](https://github.com/bobishh/ecky/commit/ea66f93096e77f5dd07a7948734e3b5703566ad7))
* **agent-relay-telepathy:** relay non-primary agent bubbles through Ecky in MCP ([e26f4f2](https://github.com/bobishh/ecky/commit/e26f4f273a2239c7e8fe9ac51d64a2b114541d7d))
* **agent:** add single-source language reference builder for API mode ([f74825f](https://github.com/bobishh/ecky/commit/f74825f1fc0eec573343988071d0b97412bb668c))
* **authoring-error-surface:** add AuthoringError type and one-way AppError conversion ([1095f56](https://github.com/bobishh/ecky/commit/1095f56eb662b9a16cbfe4ac562dd78af6ef5dc5))
* **authoring-error-surface:** add nearest-op suggester over Core IR vocabulary ([70a8ebd](https://github.com/bobishh/ecky/commit/70a8ebdb5982d02d16b755af295439a3dcf745a2))
* **compiler:** surface selector parse errors and add semantic AST baseline guards ([5cc9c1e](https://github.com/bobishh/ecky/commit/5cc9c1ef399dcfcd1ad94b0ee99bc5bb7d447877))
* **docs:** server-render Ecky IR field guide as themed web page ([9f1cc0b](https://github.com/bobishh/ecky/commit/9f1cc0bc6ea323eca97267b894b4fc7b143580eb))
* **ecky:** convenience ops + feature ops + freecad transpiler spec [WIP] ([d1acdab](https://github.com/bobishh/ecky/commit/d1acdab9fb4f465fbd6cf54523a7457d25080892))
* **frontend:** enhance AST map editor, drawing overlay, and add macro diff panel ([dccc577](https://github.com/bobishh/ecky/commit/dccc5776380e592023e5ffc2912446fed5b3f356))
* **landing:** add ecky-cad.com static landing + docs serving ([23854f3](https://github.com/bobishh/ecky/commit/23854f3d667960f2959f1b5a01c50c91ee789af0))
* **landing:** reuse app-icon as favicon + apple-touch-icon ([bf6c540](https://github.com/bobishh/ecky/commit/bf6c5402114b609fc5904687c92e802deb55df99))
* **native-occt:** add draft angle, hull, SVG wire-soup, and build123d parity ([593cc55](https://github.com/bobishh/ecky/commit/593cc55b7e5b060f3c1a26339b8fa03f8b1d693b))
* **session:** add session activity diff view and param-change emission ([cb6fa89](https://github.com/bobishh/ecky/commit/cb6fa891e894069e885c0eba00cb4b14d20e1286))
* **transpile:** llm cad-to-ecky path, retire deterministic freecad walker ([72ce142](https://github.com/bobishh/ecky/commit/72ce1421cf66e8cacca6d3958508d435647c8d5c))


### Bug Fixes

* **compiler:** reject (define ...) inside (model ...) with clear let* hint ([06e5269](https://github.com/bobishh/ecky/commit/06e52692c50c8a70be3a3e83a45f8c0e5af12030))
* **compiler:** surface real parse errors with line numbers ([434a3f3](https://github.com/bobishh/ecky/commit/434a3f35c97bef85469bf84d2ceabbd9955156a8))
* **docs:** correct content-type and redirect scheme for /docs ([2fd04be](https://github.com/bobishh/ecky/commit/2fd04be85927f798a2b93fa9f43cc45e2172f5de))
* **landing:** link docs with trailing slash to bypass cached 301 ([b7889ae](https://github.com/bobishh/ecky/commit/b7889ae753fba7ece56b1e08669a3d756482579e))
* **native-occt:** emit per-part binary STL for multipart export ([3fdce32](https://github.com/bobishh/ecky/commit/3fdce3299a6c39c1106ccc6816ff165c6f13e3d3))
* **occt:** flatten bezier-path in runner to restore native SVG render ([415ecfc](https://github.com/bobishh/ecky/commit/415ecfc29570235ded151b55c23f30d8e30308f5))
* **occt:** radial helical thread via frenet trihedron + right-corner transition ([886be04](https://github.com/bobishh/ecky/commit/886be0463a7e0110549cfa48f6c8dac445175354))
* **render:** config is authoritative for new thread dialect/backend ([f565632](https://github.com/bobishh/ecky/commit/f5656321d0ffa7fb1323edd9a1a4af5e69053fd5))
* **render:** finer OCCT tessellation for smoother curved surfaces ([221657e](https://github.com/bobishh/ecky/commit/221657ef4f4d5030f3419f5a1c46c9435bc4c6a2))
* **render:** route mesh-only ops to mesh renderer, strip error noise ([c096695](https://github.com/bobishh/ecky/commit/c096695247a6d2ea03b797a11492217d9a853d15))
* **thread:** bury ridge root in core to stop coincident-face hollow bug ([7f761e6](https://github.com/bobishh/ecky/commit/7f761e6003db269bf10855e78c8a511c15f17975))


### Refactoring

* decompose contracts and mcp handlers into per-domain modules ([9093476](https://github.com/bobishh/ecky/commit/909347688e0c08982bdc229041f3b4d3d95198af))
* **render:** dispatch sampled-radial-loft and hull natively, tighten prompts ([4561232](https://github.com/bobishh/ecky/commit/4561232ce98f7f0db977cbd0f4f88e0a3bbb10c2))


### Documentation

* **authoring-error-surface:** add self-teaching error surface change ([0ff442d](https://github.com/bobishh/ecky/commit/0ff442d8bab3f6797f84cf647be83ff51d9058d2))
* **authoring-error-surface:** retire How Ecky Thinks band-aid so the guide opens on the first model ([9da0c11](https://github.com/bobishh/ecky/commit/9da0c111a151f625f2af74bcebc12b4b7f9c144c))
* **ecky-ir-book:** add loft/offset/scale examples and rendered illustrations ([bedae8d](https://github.com/bobishh/ecky/commit/bedae8deab576baa3a8c674df8873baf51936bf2))
* **parametric-thread-feature:** spec thread as a structural primitive ([862e93e](https://github.com/bobishh/ecky/commit/862e93e59419b21bece38feafa3e28b347f86833))
* require english commit messages ([559d2a0](https://github.com/bobishh/ecky/commit/559d2a0d2cd7b754a0064ef49e30cb804f20a85d))


### Build System

* adopt conventional commits and release please ([63b3fdd](https://github.com/bobishh/ecky/commit/63b3fdd408491f26d3c205a1035b96a466fb741d))
