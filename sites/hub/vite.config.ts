import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
const root=fileURLToPath(new URL('.',import.meta.url));
const repoRoot=resolve(root,'../..');
export default defineConfig({root,plugins:[svelte()],resolve:{alias:{'@genome':resolve(repoRoot,'src/lib/genie')}},server:{host:'127.0.0.1',port:5198,strictPort:true,fs:{allow:[repoRoot]},proxy:{'/v1':process.env.HUB_REGISTRY_URL ?? 'http://127.0.0.1:8247'}},build:{outDir:'dist',emptyOutDir:true,target:'es2020',chunkSizeWarningLimit:800}});
