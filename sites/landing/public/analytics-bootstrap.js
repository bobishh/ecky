(() => {
  if (window.location.hostname.toLowerCase() !== 'ecky-cad.com') return;

  const script = document.createElement('script');
  script.defer = true;
  script.src = 'https://analytics.meta-uber-engineer.dev/client.js';
  script.dataset.project = 'ecky-cad';
  script.dataset.token = '216c3f984f307273268ca7e98b97a35f4b14965897d06f872f0e267cbee18b6c';
  document.head.append(script);
})();
