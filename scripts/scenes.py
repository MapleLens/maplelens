"""Labelled synthetic scenes for the detection benchmarks, drawn by mscw-render (the game client's own engine,
offline, pixel-identical to the client): item tooltips over many maps, inventory and store hovers, and pairs of
owner-view store frames (before / after visitors, chat and sales) with the names on them.

    python -I scripts/scenes.py OUT_DIR --count 5000 [--server http://host:8765] [--workers 3] [--start 0]

Each scene is OUT_DIR/<kind>/<n>.png (+ <n>b.png for the second store frame) and a line of OUT_DIR/labels.jsonl:
{"id", "kind", "files", "display", "mouse", "item", "owner", "events", ...}. Items come from the local price snapshot
(data/list.json) when there is one, so every tooltip names an item the catalog knows; the snapshot is only read
here, never written out. Scenes already in labels.jsonl are skipped, so a stopped run resumes.
"""

import argparse
import base64
import json
import os
import random
import sys
import threading
import urllib.request
from concurrent.futures import ThreadPoolExecutor

RESOLUTIONS = [
    (800, 600), (1024, 768), (1280, 720), (1280, 800), (1366, 768), (1440, 900), (1600, 900), (1680, 1050),
    (1920, 1080), (1920, 1200), (2560, 1080), (2560, 1440), (3440, 1440),
]
# Most players run 1080p-class screens; the small and huge ones still get their share.
RES_WEIGHTS = [1, 3, 3, 2, 5, 2, 3, 2, 10, 5, 2, 4, 1]
UI_SIZES = ["default", "filtered", "ideal"]
UI_WEIGHTS = [5, 1, 4]
FREE_MARKET = list(range(80002001, 80002021))
JOBS = ["Beginner", "Warrior", "Magician", "Bowman", "Thief", "Rogue", "Fighter", "Page", "Spearman", "Cleric",
        "Wizard", "Hunter", "Crossbowman", "Assassin", "Bandit", "Pirate"]
SYLLABLES = ["ka", "ri", "mo", "zen", "lu", "ra", "vyn", "dax", "el", "il", "li", "ol", "qu", "x", "sh", "ty", "ae",
             "rn", "m", "w", "vv", "nn", "ii", "ll", "ck", "gg", "ph", "yy", "jo", "fe", "ti", "ft", "d", "b", "p", "q"]
CHAT = ["hi", "how much for the scroll?", "is this still here", "can u do 50k?", "ty!", "lol", "brb", "selling cheap",
        "B> Dark Scroll", "S> Ilbi 2m", "where is henesys", "nice store", "ok", "anyone have elixirs?", "gl",
        "do you have more?", "pm me", "thx", "wow", "100k each?", "i'll take 2", "coming back later", "100% gloves?"]


NATURAL = ["Kaedos", "Ravyn", "Smoggy", "Lilith", "Illidan", "Mapler", "Wolfie", "Bambi", "Cloudy", "Dragonslayer",
           "Mimi", "Luna", "Kiwi", "Momo", "Rinrin", "Yuki", "Sora", "Haru", "Bunny", "Nova", "Zero", "Ilbi", "Lucky",
           "Pinky", "Shadow", "Blaze", "Frost", "Kitty", "Panda", "Tofu", "Mochi", "Boba", "Nyx", "Vex", "Jinx"]


def name(rng):
    if rng.random() < 0.35:
        text = rng.choice(NATURAL)
        if rng.random() < 0.5:
            text += str(rng.randint(1, 9999))
        if rng.random() < 0.2:
            text = text.lower()
        return text[:12]
    return odd_name(rng)


def odd_name(rng):
    """A character name: 4-12 letters and digits, with the look-alikes (I/l, rn/m, vv/w) names really use."""
    while True:
        parts = [rng.choice(SYLLABLES) for _ in range(rng.randint(2, 5))]
        text = "".join(parts)
        if rng.random() < 0.5:
            text = text.capitalize()
        if rng.random() < 0.15:
            text = text.upper()
        if rng.random() < 0.3:
            text += str(rng.randint(0, 999))
        if rng.random() < 0.15:
            text = "".join(c.upper() if rng.random() < 0.3 else c for c in text)
        text = text[:12]
        if 4 <= len(text) <= 12 and text[0].isalpha():
            return text


def display(rng):
    w, h = rng.choices(RESOLUTIONS, RES_WEIGHTS)[0]
    return {"width": w, "height": h, "ui_size": rng.choices(UI_SIZES, UI_WEIGHTS)[0]}


def ui_px(d):
    """Screen pixels per UI pixel, as the client draws its windows."""
    if d["width"] < 1920 or d["height"] < 1080:
        return 1.0
    if d["ui_size"] == "ideal":
        return 2.0 if d["height"] >= 1536 else 1.0
    return d["height"] / 768


def mouse_in(rng, d, pos, box):
    """A cursor inside a window at `pos` (screen px), `box` = (x0, y0, x1, y1) in UI px of that window."""
    s = ui_px(d)
    return {"x": min(d["width"] - 2, pos["x"] + round(rng.uniform(box[0], box[2]) * s)),
            "y": min(d["height"] - 2, pos["y"] + round(rng.uniform(box[1], box[3]) * s))}


def hud(rng, who=None):
    return {"name": who or name(rng), "level": rng.randint(1, 200), "job": rng.choice(JOBS)}


class Renderer:
    def __init__(self, server):
        self.server = server.rstrip("/")

    def post(self, path, body):
        body = dict(body, response="json")
        request = urllib.request.Request(self.server + path, json.dumps(body).encode(),
                                         {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=300) as response:
            return json.loads(response.read())

    def get(self, path):
        with urllib.request.urlopen(self.server + path, timeout=60) as response:
            return json.loads(response.read())


def snapshot_items(renderer, cache):
    """(id, name, category) of the price list's items the renderer can draw."""
    if os.path.exists(cache):
        return json.load(open(cache, encoding="utf8"))
    data = json.load(open("data/list.json", encoding="utf8"))
    out = []
    for item in data["items"]:
        try:
            item_id = int(item["id"].split(":")[1])
            known = renderer.get(f"/v1/items/{item_id}")
        except Exception:
            continue
        out.append([item_id, known["name"], item.get("category"), known.get("kind")])
    json.dump(out, open(cache, "w", encoding="utf8"))
    return out


def maps(renderer, cache):
    if os.path.exists(cache):
        return json.load(open(cache, encoding="utf8"))
    seen = {}
    for q in "aeiouy":
        for m in renderer.get(f"/v1/maps?search={q}&limit=1000"):
            seen[m["map_id"]] = m
    out = list(seen.values())
    json.dump(out, open(cache, "w", encoding="utf8"))
    return out


TAB = {"equip": "equip", "consume": "use", "install": "setup", "etc": "etc", "cash": "cash"}


def tooltip_scene(rng, items, all_maps, sid):
    item_id, item_name, _, kind = rng.choice(items)
    d = display(rng)
    mouse = {"x": rng.randint(0, d["width"] - 60), "y": rng.randint(0, d["height"] - 60)}
    body = {"display": d, "seed": sid, "map_id": rng.choice(all_maps)["map_id"], "item_id": item_id,
            "mouse": mouse, "random_players": rng.choice([0, 0, 2, 5]), "random_booths": rng.choice([0, 0, 3]),
            "hud": hud(rng), "animation_time_ms": rng.randint(0, 60000)}
    if kind == "equip" and rng.random() < 0.25:
        body["upgrade"] = {"scrolls": rng.randint(1, 7)}
        if rng.random() < 0.3:
            body["upgrade"]["crafter"] = name(rng)
    return "/v1/render/item-tooltip", body, {"item": item_name, "item_id": item_id, "mouse": mouse}


def inventory_scene(rng, items, all_maps, sid):
    tab_kind = rng.choice(["equip", "consume", "etc", "install"])
    pool = [i for i in items if i[3] == tab_kind] or items
    picks = rng.sample(pool, min(len(pool), rng.randint(1, 12)))
    slots = rng.sample(range(24), len(picks))
    filled = [{"slot": s, "item_id": i[0], "quantity": 1 if tab_kind == "equip" else rng.randint(1, 300)}
              for s, i in zip(slots, picks)]
    k = rng.randrange(len(filled))
    d = display(rng)
    pos = {"x": rng.randint(0, max(1, d["width"] - 400)), "y": rng.randint(0, max(1, d["height"] - 450))}
    body = {"display": d, "seed": sid, "map_id": rng.choice(all_maps)["map_id"], "hud": hud(rng),
            "random_players": rng.choice([0, 3]),
            "inventory": {"tab": TAB[tab_kind], "slots": filled, "hover_slot": filled[k]["slot"], "position": pos,
                          "mouse": mouse_in(rng, d, pos, (15, 50, 190, 250))}}
    label = {"item": picks[k][1], "item_id": picks[k][0], "mouse": body["inventory"]["mouse"]}
    return "/v1/render/inventory", body, label


def store_body(rng, items, sid, role, d, owner, visitors, chat, sold, meso):
    shop_maps = FREE_MARKET
    store = {"role": role, "booth": 0, "visitors": visitors, "chat": chat, "sold": sold, "meso": meso,
             "items": [{"item_id": i[0], "quantity": 1, "bundles": rng.randint(1, 3),
              "price": rng.choice([1, 999, 50000, 1000000, rng.randint(1, 99_999_999)])}
                       for i in rng.sample(items, rng.randint(1, 6))]}
    if role == "visitor" or rng.random() < 0.6:
        store["position"] = {"x": rng.randint(0, max(1, d["width"] - 1100)),
                             "y": rng.randint(0, max(1, d["height"] - 700))}
    body = {"display": d, "seed": sid, "map_id": rng.choice(shop_maps), "booths": [{"owner": owner}],
            "random_booths": rng.randint(0, 10), "random_players": rng.choice([0, 2, 4]), "store": store}
    return body


def store_scene(rng, items, all_maps, sid):
    """Two frames of our own open store: what was there, then visitors, chat and sales since."""
    d = display(rng)
    owner = name(rng)
    people = [name(rng) for _ in range(6)]
    base_chat, events, new_chat = [], [], []
    for _ in range(rng.randint(0, 4)):
        base_chat.append(f"{rng.choice(people)} has entered.")
    inside = []
    for _ in range(rng.randint(1, 5)):
        r = rng.random()
        if r < 0.4 or not inside:
            who = rng.choice(people)
            new_chat.append(f"{who} has entered.")
            events.append(["entered", who])
            inside.append(who)
        elif r < 0.7:
            who = inside.pop(rng.randrange(len(inside)))
            new_chat.append(f"{who} has left the room.")
            events.append(["left", who])
        else:
            who = rng.choice(inside)
            text = rng.choice(CHAT)
            new_chat.append(f"{who} : {text}")
            events.append(["chat", f"{who} : {text}"])
    base_sold = [{"item_id": i[0], "buyer": name(rng), "number": rng.randint(1, 200), "price": rng.randint(1, 99999)}
                 for i in rng.sample(items, 1)]  # an empty list would be drawn as random rows
    new_sold = [{"item_id": i[0], "buyer": rng.choice(people), "number": rng.randint(1, 200),
                 "price": rng.randint(1, 99999)} for i in rng.sample(items, rng.randint(0, 2))]
    names = {i[0]: i[1] for i in items}
    for s in new_sold:
        events.append(["sale", names[s["item_id"]], s["buyer"], s["number"]])
    meso = rng.randint(0, 2_000_000_000)
    visitors = inside[:3]
    a = store_body(rng, items, sid, "owner", d, owner, [], base_chat, base_sold, meso)
    b = json.loads(json.dumps(a))
    b["store"]["chat"] = base_chat + new_chat
    b["store"]["sold"] = new_sold + base_sold
    b["store"]["visitors"] = visitors
    b["store"]["meso"] = meso + sum(s["number"] * s["price"] * 97 // 100 for s in new_sold)
    b.pop("seed")
    b["seed"] = sid
    return [("/v1/render/permit-store", a), ("/v1/render/permit-store", b)], {"owner": owner, "events": events}


def visitor_scene(rng, items, all_maps, sid):
    """Someone else's store, seen as a visitor: nothing may be reported, and a hovered row is an item tooltip."""
    d = display(rng)
    owner, me = name(rng), name(rng)
    chat = [f"{me} has entered."] + [f"{me} : {rng.choice(CHAT)}" for _ in range(rng.randint(0, 2))]
    body = store_body(rng, items, sid, "visitor", d, owner, [me], chat, [], rng.randint(0, 10**8))
    body["hud"] = hud(rng, me)
    hover = rng.random() < 0.7
    label = {"owner": owner, "events": []}
    if hover:
        row = rng.randrange(len(body["store"]["items"]))
        body["store"]["hover_row"] = row
        body["store"]["sold_panel"] = False
        item_id = body["store"]["items"][row]["item_id"]
        label["item"] = next(i[1] for i in items if i[0] == item_id)
        label["item_id"] = item_id
        body["store"]["mouse"] = mouse_in(rng, d, body["store"]["position"], (10, 160, 300, 520))
        label["mouse"] = body["store"]["mouse"]
    return "/v1/render/permit-store", body, label


KINDS = [("tooltip", tooltip_scene, 40), ("inventory", inventory_scene, 15), ("store", store_scene, 35),
         ("visitor", visitor_scene, 10)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out")
    parser.add_argument("--count", type=int, default=5000)
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--server", default="http://192.168.2.225:8765")
    parser.add_argument("--workers", type=int, default=3)
    args = parser.parse_args()
    renderer = Renderer(args.server)
    os.makedirs(args.out, exist_ok=True)
    items = snapshot_items(renderer, os.path.join(args.out, "items.json"))
    all_maps = maps(renderer, os.path.join(args.out, "maps.json"))
    print(f"{len(items)} items, {len(all_maps)} maps", flush=True)
    labels_path = os.path.join(args.out, "labels.jsonl")
    done = set()
    if os.path.exists(labels_path):
        done = {json.loads(line)["id"] for line in open(labels_path, encoding="utf8") if line.strip()}
    lock = threading.Lock()
    labels = open(labels_path, "a", encoding="utf8")
    counter = [0]

    def make(sid):
        rng = random.Random(sid * 7919 + 13)
        kind, fn, _ = rng.choices(KINDS, [k[2] for k in KINDS])[0]
        made = fn(rng, items, all_maps, sid)
        renders, label = (made[0], made[1]) if kind == "store" else ([(made[0], made[1])], made[2])
        folder = os.path.join(args.out, kind)
        os.makedirs(folder, exist_ok=True)
        files, texts = [], []
        for k, (path, body) in enumerate(renders):
            for attempt in range(3):
                try:
                    result = renderer.post(path, body)
                    break
                except Exception as error:  # a 400 for a scene with no room is skipped, not retried
                    if "400" in str(error) or attempt == 2:
                        print(f"{sid}: {error}", file=sys.stderr, flush=True)
                        return
            file = f"{kind}/{sid}{'b' if k else ''}.png"
            open(os.path.join(args.out, file), "wb").write(base64.b64decode(result["image_png_base64"]))
            files.append(file)
            texts.append([[t["text"], t["bbox"], t["color"]] for t in result["labels"]["texts"] if t["visible"]])
        label.update(id=sid, kind=kind, files=files, display=renders[0][1]["display"], texts=texts)
        with lock:
            labels.write(json.dumps(label) + "\n")
            labels.flush()
            counter[0] += 1
            if counter[0] % 50 == 0:
                print(f"{counter[0]} scenes", flush=True)

    todo = [sid for sid in range(args.start, args.start + args.count) if sid not in done]
    with ThreadPoolExecutor(args.workers) as pool:
        list(pool.map(make, todo))


if __name__ == "__main__":
    main()
