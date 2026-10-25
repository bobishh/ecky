import { expect, test } from '@playwright/test';
const digest = 'sha256:' + 'a'.repeat(64);
const release = {
  packageId:'ecky.fasteners', version:'1.0.0', displayName:'Ecky fasteners', archiveDigest:digest,
  packageDigest:'sha256:'+'b'.repeat(64), yanked:false, distribution:{license:'MIT',dependencies:[]},
  header:{packageId:'ecky.fasteners',version:'1.0.0',displayName:'Ecky fasteners',tags:['mechanical'],components:[{componentId:'washer',displayName:'Washer',params:[{key:'inner-d',kind:'number',unit:'mm'}],ports:[]}]},
};
test('Given published exports When searched and inspected Then exact source and installation instructions are available',async({page})=>{
  await page.route('**/v1/search?*', route=>route.fulfill({json:{releases:[release]}}));
  await page.route('**/v1/packages/ecky.fasteners/1.0.0/components/washer',route=>route.fulfill({json:{source:'(define-component washer () (box 2 2 1))',archiveDigest:digest,license:'MIT'}}));
  await page.goto('/');
  await expect(page.getByRole('heading',{name:'Reusable stones. Real components.'})).toBeVisible();
  await page.getByRole('searchbox',{name:'Search components and packages'}).fill('washer');
  await page.getByRole('button',{name:'SEARCH',exact:true}).click();
  await page.getByRole('button',{name:'INSPECT Washer',exact:true}).click();
  await expect(page.locator('pre').filter({hasText:'define-component washer'})).toBeVisible();
  await expect(page.getByText('cairn add ecky.fasteners@1.0.0',{exact:true})).toBeVisible();
  await expect(page.getByRole('link',{name:'DOWNLOAD PACKAGE'})).toHaveAttribute('href',`/v1/blobs/${encodeURIComponent(digest)}`);
  if (process.env.HUB_SCREENSHOT_PATH) await page.screenshot({path:process.env.HUB_SCREENSHOT_PATH,fullPage:true});
  await page.setViewportSize({width:390,height:844});
  await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
});
test('Given registry is unavailable When search fails Then raw error and retry remain visible',async({page})=>{
  let attempts=0;
  await page.route('**/v1/search?*',route=>{attempts++;return attempts===1?route.fulfill({status:503,json:{error:'Registry maintenance in progress'}}):route.fulfill({json:{releases:[release]}})});
  await page.goto('/');
  await expect(page.getByText('Registry maintenance in progress',{exact:true})).toBeVisible();
  await page.getByRole('button',{name:'RETRY',exact:true}).click();
  await expect(page.getByRole('heading',{name:'Ecky fasteners',exact:true})).toBeVisible();
});
