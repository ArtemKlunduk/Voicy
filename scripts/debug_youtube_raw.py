"""Сравнить raw HTTP fetch (как Rust ureq делает) с Playwright (как браузер видит)."""
import re
import sys
import urllib.parse
import urllib.request

QUERY = sys.argv[1] if len(sys.argv) > 1 else "анютка асмр"
URL = f"https://www.youtube.com/results?search_query={urllib.parse.quote_plus(QUERY)}&hl=en"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"

print(f"Query: {QUERY}")
print(f"URL: {URL}\n")

req = urllib.request.Request(URL, headers={"User-Agent": UA})
with urllib.request.urlopen(req, timeout=10) as r:
    html = r.read().decode("utf-8", errors="replace")
print(f"HTML size: {len(html) / 1024:.1f} KB")

# Тот же паттерн что у Rust
needle = '"videoRenderer":{"videoId":"'
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

print(f"\n[Raw ureq + same regex] Found {len(rust_ids)} unique videoIds")
for i, vid in enumerate(rust_ids[:10]):
    print(f"  #{i+1}: https://youtu.be/{vid}")

# Достанем заголовки чтобы понять что есть рядом с каждым ID
print("\n[Context around first 5 hits — что окружает videoId в HTML]")
pos = 0
count = 0
while count < 5:
    idx = html.find(needle, pos)
    if idx == -1:
        break
    after = idx + len(needle)
    vid = html[after:after + 11]
    # Найти "title":"text" в следующих 800 символах
    context = html[after:after + 1500]
    m_title = re.search(r'"title":\{"runs":\[\{"text":"([^"]+)"', context) or \
              re.search(r'"title":\{"simpleText":"([^"]+)"', context)
    m_owner = re.search(r'"ownerText":\{"runs":\[\{"text":"([^"]+)"', context) or \
              re.search(r'"longBylineText":\{"runs":\[\{"text":"([^"]+)"', context)
    title = m_title.group(1) if m_title else "(no title found)"
    owner = m_owner.group(1) if m_owner else "?"
    print(f"  #{count+1}: {vid}  «{title[:60]}»  by {owner[:30]}")
    count += 1
    pos = after + 11
