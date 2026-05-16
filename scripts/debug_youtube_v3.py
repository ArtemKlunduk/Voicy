"""Тест: cookies + полные браузерные headers → даёт ли это ranking ближе к Playwright?"""
import json
import re
import sys
import urllib.parse
import urllib.request
import gzip

QUERY = sys.argv[1] if len(sys.argv) > 1 else "анютка асмр"
URL = f"https://www.youtube.com/results?search_query={urllib.parse.quote_plus(QUERY)}&hl=en"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"

# Cookies, которые показывают YouTube'у что мы «настоящий» консент-готовый юзер
COOKIES = "; ".join([
    "CONSENT=YES+cb.20210328-17-p0.en+FX+555",
    "VISITOR_INFO1_LIVE=Z0Z0Z0Z0Z0Z",  # рандомный visitor id
    "PREF=tz=Europe.Moscow&hl=en",
    "YSC=test_session",
])

req = urllib.request.Request(URL, headers={
    "User-Agent": UA,
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9,ru;q=0.8",
    "Accept-Encoding": "gzip, deflate",
    "Sec-Ch-Ua": '"Google Chrome";v="130", "Chromium";v="130", "Not?A_Brand";v="99"',
    "Sec-Ch-Ua-Mobile": "?0",
    "Sec-Ch-Ua-Platform": '"Windows"',
    "Sec-Fetch-Dest": "document",
    "Sec-Fetch-Mode": "navigate",
    "Sec-Fetch-Site": "none",
    "Sec-Fetch-User": "?1",
    "Upgrade-Insecure-Requests": "1",
    "Cookie": COOKIES,
})

print(f"Query: {QUERY}\nURL: {URL}\n")

with urllib.request.urlopen(req, timeout=10) as r:
    data = r.read()
    enc = r.headers.get("Content-Encoding", "")
    if enc == "gzip":
        data = gzip.decompress(data)
    html = data.decode("utf-8", errors="replace")

print(f"HTML size: {len(html) / 1024:.1f} KB\n")

# JSON walk
m = re.search(r'ytInitialData\s*=\s*({.+?});\s*</script>', html, re.DOTALL) or \
    re.search(r'var ytInitialData = ({.+?});', html)

if m:
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

    print(f"[With cookies+browser headers] {len(organic)} results")
    for i, (vid, title) in enumerate(organic[:10]):
        print(f"  #{i+1}: {vid}  «{title[:55]}»")
