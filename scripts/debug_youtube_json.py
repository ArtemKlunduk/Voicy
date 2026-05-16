"""Парсим ytInitialData правильно из raw HTTP — не regex, а JSON walk.
Проверяем: даст ли это тот же порядок что Playwright (real browser)?"""
import json
import re
import sys
import urllib.parse
import urllib.request

QUERY = sys.argv[1] if len(sys.argv) > 1 else "анютка асмр"
URL = f"https://www.youtube.com/results?search_query={urllib.parse.quote_plus(QUERY)}&hl=en"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"

# Дополнительные headers как у настоящего браузера
req = urllib.request.Request(URL, headers={
    "User-Agent": UA,
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9,ru;q=0.8",
    "Accept-Encoding": "gzip, deflate",
    "Sec-Fetch-Dest": "document",
    "Sec-Fetch-Mode": "navigate",
    "Sec-Fetch-Site": "none",
    "Sec-Fetch-User": "?1",
    "Upgrade-Insecure-Requests": "1",
})

print(f"Query: {QUERY}")
print(f"URL: {URL}\n")

with urllib.request.urlopen(req, timeout=10) as r:
    data = r.read()
    if r.headers.get("Content-Encoding") == "gzip":
        import gzip
        data = gzip.decompress(data)
    html = data.decode("utf-8", errors="replace")

print(f"HTML size: {len(html) / 1024:.1f} KB\n")

# ── Способ A: точный regex как у Rust ────────────────────────
needle = '"videoRenderer":{"videoId":"'
regex_ids = []
pos = 0
while True:
    idx = html.find(needle, pos)
    if idx == -1:
        break
    after = idx + len(needle)
    id_ = html[after:after + 11]
    if id_ not in regex_ids:
        regex_ids.append(id_)
    pos = after + 11

print(f"[A: raw regex] {len(regex_ids)} IDs")
for i, vid in enumerate(regex_ids[:7]):
    print(f"  #{i+1}: {vid}")

# ── Способ B: JSON walk через ytInitialData ──────────────────
m = re.search(r'ytInitialData\s*=\s*({.+?});\s*</script>', html, re.DOTALL)
if not m:
    m = re.search(r'var ytInitialData = ({.+?});', html)

if m:
    try:
        data = json.loads(m.group(1))
        organic = []
        sections = (data.get("contents", {})
                       .get("twoColumnSearchResultsRenderer", {})
                       .get("primaryContents", {})
                       .get("sectionListRenderer", {})
                       .get("contents", []))
        for sec in sections:
            items = sec.get("itemSectionRenderer", {}).get("contents", [])
            for it in items:
                vr = it.get("videoRenderer")
                if vr and "videoId" in vr:
                    title_runs = vr.get("title", {}).get("runs", [])
                    title = title_runs[0].get("text", "") if title_runs else ""
                    organic.append((vr["videoId"], title))
        print(f"\n[B: JSON walk organic] {len(organic)} results")
        for i, (vid, title) in enumerate(organic[:7]):
            print(f"  #{i+1}: {vid}  «{title[:55]}»")
    except json.JSONDecodeError as e:
        print(f"[B] JSON parse failed: {e}")
else:
    print("[B] ytInitialData not found in HTML")

# ── Способ C: сравнение позиций по AB ────────────────────────
print("\n[Сравнение A regex vs B JSON walk]")
for i in range(min(7, len(regex_ids), len(organic) if 'organic' in dir() else 0)):
    a = regex_ids[i]
    b = organic[i][0] if i < len(organic) else "—"
    flag = "✓" if a == b else "✗ DIFF"
    print(f"  #{i+1}: regex={a}  json={b}  {flag}")
