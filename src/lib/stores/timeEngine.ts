import { readable } from 'svelte/store';

/**
 * A global, headless clock that updates every second.
 * Automatically starts the interval when the first subscriber attaches,
 * and clears it when the last subscriber detaches.
 */
export const nowSeconds = readable(Math.floor(Date.now() / 1000), (set) => {
  const interval = setInterval(() => {
    set(Math.floor(Date.now() / 1000));
  }, 1000);

  return () => clearInterval(interval);
});
