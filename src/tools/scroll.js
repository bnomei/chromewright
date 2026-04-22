(async function () {
  const config = __SCROLL_CONFIG__;
  const beforeScrollY = window.scrollY;
  const scrollAmount =
    typeof config.amount === "number" ? config.amount : window.innerHeight;

  window.scrollBy(0, scrollAmount);

  await new Promise((resolve) => setTimeout(resolve, 100));

  const actualScroll = window.scrollY - beforeScrollY;
  const scrollHeight = Math.max(
    document.documentElement ? document.documentElement.scrollHeight : 0,
    document.body ? document.body.scrollHeight : 0
  );
  const scrollTop = window.scrollY;
  const clientHeight =
    window.innerHeight ||
    (document.documentElement ? document.documentElement.clientHeight : 0);
  const isAtTop = Math.abs(scrollTop) <= 1;
  const isAtBottom =
    Math.abs(scrollHeight - scrollTop - clientHeight) <= 1;

  return JSON.stringify({ actualScroll, isAtBottom, scrollY: scrollTop, isAtTop });
})()
