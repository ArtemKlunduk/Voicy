"""
Debug: какие videoId извлекает наш Rust-парсер vs реальный порядок
органических результатов на YouTube search.

Запуск:
    python scripts/debug_youtube_selectors.py "анютка асмр"
"""
import asyncio
import json
import re
import sys
from playwright.async_api import async_playwright


QUERY = sys.argv[1] if len(sys.argv) > 1 else "lo-fi beats"
URL = f"https://www.youtube.com/results?search_query={QUERY.replace(' ', '+')}&hl=en"


async def main():
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=True)
        ctx = await browser.new_context(
            user_agent="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                       "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        )
        page = await ctx.new_page()
        print(f"\n→ Visiting {URL}")
        await page.goto(URL, wait_until="networkidle", timeout=30000)

        # Принять cookies если есть consent-баннер
        try:
            await page.click('button[aria-label*="Accept" i]', timeout=2000)
        except Exception:
            pass
        await page.wait_for_timeout(1500)

        html = await page.content()
        print(f"  HTML size: {len(html) / 1024:.1f} KB")

        # ── Способ 1: точно как наш Rust код ─────────────────────────
        needle = r'"videoRenderer":{"videoId":"'
        rust_ids = []
        pos = 0
        while True:
            idx = html.find(needle, pos)
            if idx == -1:
                break
            after = idx + len(needle)
            id_ = html[after:after + 11]
            if id_ not in rust_ids:
                rust_ids.append(id_)
            pos = after + 11

        print(f"\n[Rust regex] Found {len(rust_ids)} unique videoIds via '\"videoRenderer\":{{\"videoId\":\"...'")
        for i, vid in enumerate(rust_ids[:10]):
            print(f"  #{i+1}: https://youtu.be/{vid}")

        # ── Способ 2: парсим ytInitialData JSON корректно ────────────
        m = re.search(r'var ytInitialData = ({.+?});</script>', html)
        if not m:
            m = re.search(r'ytInitialData = ({.+?});\s*(?:window|var|</script>)', html)
        if m:
            data = json.loads(m.group(1))
            organic = []
            try:
                sections = data["contents"]["twoColumnSearchResultsRenderer"][
                    "primaryContents"]["sectionListRenderer"]["contents"]
                for sec in sections:
                    items = sec.get("itemSectionRenderer", {}).get("contents", [])
                    for it in items:
                        vr = it.get("videoRenderer")
                        if vr and "videoId" in vr:
                            title = vr.get("title", {}).get("runs", [{}])[0].get("text", "")
                            channel = (vr.get("ownerText", {}).get("runs", [{}])[0].get("text", "")
                                       or vr.get("longBylineText", {}).get("runs", [{}])[0].get("text", ""))
                            organic.append((vr["videoId"], title, channel))
                        elif "compactVideoRenderer" in it:
                            pass  # related, не результат
                        elif "promotedSparklesTextSearchRenderer" in it or "adSlotRenderer" in it:
                            pass  # реклама
            except (KeyError, TypeError) as e:
                print(f"\n[ytInitialData] Failed to parse: {e}")
                organic = []

            print(f"\n[ytInitialData JSON] Found {len(organic)} organic results")
            for i, (vid, title, channel) in enumerate(organic[:10]):
                print(f"  #{i+1}: {vid}  «{title[:60]}»  by {channel[:30]}")

        # ── Способ 3: реальные DOM-ссылки на странице ────────────────
        dom_results = await page.evaluate("""() => {
            const items = document.querySelectorAll('ytd-video-renderer');
            return Array.from(items).slice(0, 10).map((el, i) => {
                const a = el.querySelector('a#video-title, a#thumbnail');
                const titleEl = el.querySelector('#video-title');
                const chanEl = el.querySelector('ytd-channel-name a');
                let href = a ? a.getAttribute('href') : null;
                let videoId = null;
                if (href) {
                    const m = href.match(/[?&]v=([^&]+)/);
                    if (m) videoId = m[1];
                }
                return {
                    index: i + 1,
                    videoId,
                    title: titleEl ? titleEl.textContent.trim().slice(0, 60) : null,
                    channel: chanEl ? chanEl.textContent.trim().slice(0, 30) : null,
                };
            });
        }""")
        print(f"\n[DOM ytd-video-renderer] Found {len(dom_results)} visible results")
        for r in dom_results:
            print(f"  #{r['index']}: {r['videoId']}  «{r['title']}»  by {r['channel']}")

        # ── Сравнение: совпадает ли первый/второй Rust с DOM ───────
        print("\n── Сравнение Rust vs реальный DOM (первые 5) ──")
        for i in range(5):
            rust_id = rust_ids[i] if i < len(rust_ids) else "—"
            dom_id = dom_results[i]['videoId'] if i < len(dom_results) and dom_results[i]['videoId'] else "—"
            match = "✓" if rust_id == dom_id else "✗ MISMATCH"
            print(f"  #{i+1}: rust={rust_id}  dom={dom_id}  {match}")

        await browser.close()


if __name__ == "__main__":
    asyncio.run(main())
