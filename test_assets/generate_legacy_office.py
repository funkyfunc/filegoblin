"""Writes the legacy (pre-2007) Office fixtures in this directory.

    python3 test_assets/generate_legacy_office.py

- legacy_report.doc  Word 97. macOS `textutil` converts the HTML below with
                     Apple's own Word 97 exporter (the one TextEdit uses).
                     Skipped elsewhere.
- legacy_deck.ppt    PowerPoint 97: three slides (title + body placeholders
                     whose text lives in the slide list, as PowerPoint writes
                     it), speaker notes on two of them, one free text box.
- legacy_ledger.xls  Excel 97 (BIFF8): two sheets, shared strings and numbers.

No writer for .ppt or .xls exists without LibreOffice or Office, so those two
are built record by record from [MS-PPT], [MS-XLS] and [MS-CFB]. The .xls
opens in macOS Quick Look (`qlmanage -p`), an independent reader, with every
value; Quick Look refuses the .ppt, whose master carries none of the formatting
records it wants. office_oxide reads both.
"""

import math
import os
import shutil
import struct
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))

# --- [MS-CFB] compound file (version 3, 512-byte sectors) --------------------

SECTOR, MINI, CUTOFF = 512, 64, 4096
END, FREE, FATSECT, NOSTREAM = 0xFFFFFFFE, 0xFFFFFFFF, 0xFFFFFFFD, 0xFFFFFFFF


def compound_file(streams):
    """`streams`: [(name, bytes)], all at the root storage."""
    mini_data, mini_fat, mini_start = bytearray(), [], {}
    big = []
    for name, data in streams:
        assert data, "zero-length streams are not used here"
        if len(data) < CUTOFF:
            first = len(mini_data) // MINI
            count = math.ceil(len(data) / MINI)
            mini_fat += [first + i + 1 for i in range(count - 1)] + [END]
            mini_data += data + bytes(count * MINI - len(data))
            mini_start[name] = first
        else:
            big.append((name, data))

    entries = 1 + len(streams)
    dir_n = math.ceil(entries * 128 / SECTOR)
    minifat_n = math.ceil(len(mini_fat) * 4 / SECTOR)
    ministream_n = math.ceil(len(mini_data) / SECTOR)
    big_n = [math.ceil(len(d) / SECTOR) for _, d in big]
    body = dir_n + minifat_n + ministream_n + sum(big_n)
    fat_n = 1
    while fat_n * (SECTOR // 4) < body + fat_n:
        fat_n += 1
    assert fat_n <= 109, "needs DIFAT sectors"

    fat = [FREE] * (fat_n * SECTOR // 4)
    next_sector = 0

    def allocate(count, marker=None):
        nonlocal next_sector
        start = next_sector
        for i in range(count):
            fat[start + i] = marker if marker else (start + i + 1 if i < count - 1 else END)
        next_sector += count
        return start if count else END

    allocate(fat_n, FATSECT)
    dir_start = allocate(dir_n)
    minifat_start = allocate(minifat_n)
    ministream_start = allocate(ministream_n)
    big_start = {name: allocate(n) for (name, _), n in zip(big, big_n)}

    # Directory: a balanced binary tree over the streams, ordered by name
    # length and then case-insensitively, as [MS-CFB] 2.6.4 requires.
    order = sorted(range(len(streams)), key=lambda i: (len(streams[i][0]), streams[i][0].upper()))
    left, right = [NOSTREAM] * len(streams), [NOSTREAM] * len(streams)

    def tree(lo, hi):
        if lo >= hi:
            return NOSTREAM
        mid = (lo + hi) // 2
        node = order[mid]
        left[node], right[node] = tree(lo, mid), tree(mid + 1, hi)
        return node + 1  # directory index; 0 is the root entry

    root_child = tree(0, len(order))

    def dir_entry(name, kind, child, lsib, rsib, start, size):
        encoded = (name + "\0").encode("utf-16-le")
        return (
            encoded.ljust(64, b"\0")
            + struct.pack("<HBB", len(encoded), kind, 1)  # black
            + struct.pack("<III", lsib, rsib, child)
            + bytes(16 + 4 + 16)  # clsid, state bits, timestamps
            + struct.pack("<IQ", start, size)
        )

    directory = dir_entry(
        "Root Entry", 5, root_child, NOSTREAM, NOSTREAM,
        ministream_start if mini_data else END, len(mini_data),
    )
    for i, (name, data) in enumerate(streams):
        start = mini_start[name] if name in mini_start else big_start[name]
        directory += dir_entry(name, 2, NOSTREAM, left[i], right[i], start, len(data))
    for _ in range(dir_n * SECTOR // 128 - entries):  # unused slots
        directory += bytes(66) + struct.pack("<BBIII", 0, 0, NOSTREAM, NOSTREAM, NOSTREAM) + bytes(48)

    def padded(data, n):
        return bytes(data) + bytes(n * SECTOR - len(data))

    header = (
        bytes.fromhex("D0CF11E0A1B11AE1") + bytes(16)
        + struct.pack("<HHHHH", 0x3E, 3, 0xFFFE, 9, 6) + bytes(6)
        + struct.pack("<IIIIIIIII", 0, fat_n, dir_start, 0, CUTOFF,
                      minifat_start if mini_fat else END, minifat_n, END, 0)
    )
    difat = list(range(fat_n)) + [FREE] * (109 - fat_n)
    header += struct.pack("<109I", *difat)
    assert len(header) == SECTOR

    out = bytearray(header)
    out += struct.pack(f"<{len(fat)}I", *fat)
    out += directory
    if minifat_n:
        out += padded(struct.pack(f"<{len(mini_fat)}I", *mini_fat), minifat_n)
    if ministream_n:
        out += padded(mini_data, ministream_n)
    for (_, data), n in zip(big, big_n):
        out += padded(data, n)
    return bytes(out)


# --- [MS-PPT] PowerPoint 97 ---------------------------------------------------


def rec(rec_type, data=b"", ver=0, inst=0):
    return struct.pack("<HHI", ver | (inst << 4), rec_type, len(data)) + data


def box(rec_type, children, inst=0):
    return rec(rec_type, b"".join(children), ver=0xF, inst=inst)


def text_atoms(text_type, text):
    """TextHeaderAtom + TextBytes/CharsAtom + a one-run StyleTextPropAtom.
    Paragraphs are separated by `\\r`."""
    try:
        chars = rec(0x0FA8, text.encode("latin-1"))
    except UnicodeEncodeError:
        chars = rec(0x0FA0, text.encode("utf-16-le"))
    runs = len(text) + 1  # the runs cover the implied final paragraph mark
    style = struct.pack("<IHI", runs, 0, 0) + struct.pack("<II", runs, 0)
    return rec(0x0F9F, struct.pack("<I", text_type)) + chars + rec(0x0FA1, style)


def shape(spid, top, left, bottom, right, client_data=(), textbox=()):
    parts = [
        rec(0xF00A, struct.pack("<II", spid, 0x0A00), ver=2, inst=1),  # rectangle
        rec(0xF010, struct.pack("<hhhh", top, left, right, bottom)),
    ]
    if client_data:
        parts.append(box(0xF011, list(client_data)))
    if textbox:
        parts.append(box(0xF00D, list(textbox)))
    return box(0xF004, parts)


def placeholder(position, kind):
    return rec(0x0BC3, struct.pack("<iBBH", position, kind, 0, 0))


def drawing(drawing_id, shapes):
    first = drawing_id * 1024
    group = box(0xF004, [
        rec(0xF009, bytes(16), ver=1),
        rec(0xF00A, struct.pack("<II", first, 0x0005), ver=2, inst=0),  # group, patriarch
    ])
    fdg = rec(0xF008, struct.pack("<II", len(shapes) + 1, first + len(shapes)), inst=drawing_id)
    return box(0x040C, [box(0xF002, [fdg, box(0xF003, [group, *shapes])])])


PT_TITLE, PT_BODY, PT_NOTES_IMAGE, PT_NOTES_BODY = 0x0D, 0x0E, 0x0B, 0x0C
TX_TITLE, TX_BODY, TX_NOTES, TX_OTHER = 0, 1, 2, 4
MASTER_ID = 0x80000000

SLIDES = [
    {
        "title": "Goblin Council Briefing",
        "body": "Agenda for the new moon\rAttendance is mandatory",
        "notes": "Welcome everyone.\rKeep it short; the troll is waiting.",
    },
    {
        "title": "Hoard Status",
        "body": "Spoons: 412\rSocks: 3,018 (unpaired)\rCrowns: 7",
        "notes": "Do not mention the crowns.",
    },
    {
        "title": "Next Steps",
        "body": "Raid the laundry\rAvoid the dog",
        "box": "Draft — for goblin eyes only",
    },
]


def powerpoint_97():
    n = len(SLIDES)
    with_notes = [i for i, s in enumerate(SLIDES) if "notes" in s]
    # Persist ids: 1 document, 2 main master, then slides, then notes pages.
    slide_pid = {i: 3 + i for i in range(n)}
    notes_pid = {i: 3 + n + k for k, i in enumerate(with_notes)}
    slide_id = {i: 0x100 + i for i in range(n)}
    notes_id = {i: 0x200 + i for i in with_notes}

    font = rec(0x0FB7, "Arial".encode("utf-16-le").ljust(64, b"\0") + bytes(4), inst=0)
    environment = box(0x03F2, [box(0x07D5, [font])])
    drawings = 1 + n + len(with_notes)
    dgg = rec(0xF006, struct.pack("<IIII", (drawings + 1) * 1024, drawings + 1, drawings * 4, drawings)
              + b"".join(struct.pack("<II", d, 4) for d in range(1, drawings + 1)))
    drawing_group = box(0x040B, [box(0xF000, [dgg])])

    masters = box(0x0FF0, [rec(0x03F3, struct.pack("<IIIIII", 2, 0, 0, 0, MASTER_ID, 0))], inst=1)
    slide_list = []
    for i, s in enumerate(SLIDES):
        slide_list.append(rec(0x03F3, struct.pack("<IIiII", slide_pid[i], 0, 2, slide_id[i], 0)))
        slide_list.append(text_atoms(TX_TITLE, s["title"]))
        slide_list.append(text_atoms(TX_BODY, s["body"]))
    notes_list = [rec(0x03F3, struct.pack("<IIIII", notes_pid[i], 0, 0, notes_id[i], 0)) for i in with_notes]

    document = box(0x03E8, [
        rec(0x03E9, struct.pack("<iiiiiiIIHHBBBB", 5760, 4320, 4320, 5760, 1, 2, 0, 0, 1, 0, 0, 0, 0, 1), ver=1),
        environment,
        drawing_group,
        masters,
        box(0x0FF0, slide_list, inst=0),
        box(0x0FF0, notes_list, inst=2),
        rec(0x03EA),
    ])

    master = box(0x03F8, [
        rec(0x03EF, struct.pack("<I8sIIHH", 0, bytes([1, 2, 0, 0, 0, 0, 0, 0]), 0, 0, 0, 0), ver=2),
        drawing(1, []),
    ])

    stream = bytearray()
    offsets = {}

    def put(pid, data):
        offsets[pid] = len(stream)
        stream.extend(data)

    put(1, document)
    put(2, master)
    for i, s in enumerate(SLIDES):
        spid = (2 + i) * 1024
        shapes = [
            shape(spid + 1, 200, 400, 900, 5360,
                  client_data=[placeholder(0, PT_TITLE)], textbox=[rec(0x0F9E, struct.pack("<i", 0))]),
            shape(spid + 2, 1100, 400, 3900, 5360,
                  client_data=[placeholder(1, PT_BODY)], textbox=[rec(0x0F9E, struct.pack("<i", 1))]),
        ]
        if "box" in s:
            shapes.append(shape(spid + 3, 4000, 400, 4250, 5360, textbox=[text_atoms(TX_OTHER, s["box"])]))
        put(slide_pid[i], box(0x03EE, [
            rec(0x03EF, struct.pack("<I8sIIHH", 1, bytes([PT_TITLE, PT_BODY, 0, 0, 0, 0, 0, 0]),
                                    MASTER_ID, notes_id.get(i, 0), 0x0007, 0), ver=2),
            drawing(2 + i, shapes),
        ]))
    for k, i in enumerate(with_notes):
        spid = (2 + n + k) * 1024
        put(notes_pid[i], box(0x03F0, [
            rec(0x03F1, struct.pack("<IHH", slide_id[i], 0x0007, 0), ver=1),
            drawing(2 + n + k, [
                shape(spid + 1, 400, 800, 3200, 3520, client_data=[placeholder(0, PT_NOTES_IMAGE)]),
                shape(spid + 2, 3400, 500, 5300, 3820, client_data=[placeholder(1, PT_NOTES_BODY)],
                      textbox=[text_atoms(TX_NOTES, SLIDES[i]["notes"])]),
            ]),
        ]))

    pids = sorted(offsets)
    persist_dir_at = len(stream)
    entry = pids[0] | (len(pids) << 20)
    stream += rec(0x1772, struct.pack(f"<I{len(pids)}I", entry, *(offsets[p] for p in pids)))
    user_edit_at = len(stream)
    stream += rec(0x0FF5, struct.pack("<IHBBIIIIHH", slide_id[0], 0, 0, 3, 0, persist_dir_at, 1,
                                      max(pids) + 1, 1, 0))

    user = "goblin"
    current_user = rec(0x0FF6, struct.pack("<IIIHHBBH", 0x14, 0xE391C05F, user_edit_at, len(user),
                                           0x03F4, 3, 0, 0)
                       + user.encode("ascii") + struct.pack("<I", 8) + user.encode("utf-16-le"))
    return compound_file([("Current User", current_user), ("PowerPoint Document", bytes(stream))])


# --- [MS-XLS] Excel 97 (BIFF8) ------------------------------------------------

SHEETS = [
    ("Hoard", [["Item", "Count", "Value"], ["Spoons", 412, 3.5], ["Socks", 3018, 0.25], ["Crowns", 7, 1200]]),
    ("Notes", [["Note"], ["Recount on Tuesday"], ["Socks are not currency"]]),
]


def biff(rec_type, data=b""):
    return struct.pack("<HH", rec_type, len(data)) + data


def bof(kind):
    return biff(0x0809, struct.pack("<HHHHII", 0x0600, kind, 0x0DBB, 0x07CC, 0, 0x06))


def excel_97():
    strings = []
    for _, rows in SHEETS:
        for row in rows:
            for v in row:
                if isinstance(v, str) and v not in strings:
                    strings.append(v)
    total = sum(isinstance(v, str) for _, rows in SHEETS for row in rows for v in row)

    sst = struct.pack("<II", total, len(strings)) + b"".join(
        struct.pack("<HB", len(s), 0) + s.encode("latin-1") for s in strings
    )
    font = biff(0x0031, struct.pack("<HHHHHBBBB", 200, 0, 0x7FFF, 400, 0, 0, 0, 0, 0)
                + struct.pack("<BB", 5, 0) + b"Arial")
    # Excel wants the 15 built-in style XFs before the first cell XF.
    style_xf = biff(0x00E0, struct.pack("<HHHBBBBIIH", 0, 0, 0xFFF5, 0x20, 0, 0, 0, 0, 0, 0x20C0))
    cell_xf = biff(0x00E0, struct.pack("<HHHBBBBIIH", 0, 0, 0x0001, 0x20, 0, 0, 0, 0, 0, 0x20C0))
    window1 = biff(0x003D, struct.pack("<hhHHHHHHH", 0, 0, 0x4000, 0x2000, 0x38, 0, 0, 1, 0x258))

    def sheet_stream(rows):
        cells = b""
        for r, row in enumerate(rows):
            for c, v in enumerate(row):
                if isinstance(v, str):
                    cells += biff(0x00FD, struct.pack("<HHHI", r, c, 15, strings.index(v)))
                else:
                    cells += biff(0x0203, struct.pack("<HHHd", r, c, 15, float(v)))
        width = max(len(row) for row in rows)
        return (bof(0x0010) + biff(0x0200, struct.pack("<IIHHH", 0, len(rows), 0, width, 0))
                + cells + biff(0x023E, struct.pack("<HHHHHHHI", 0x06B6, 0, 0, 0x40, 0, 0, 0, 0))
                + biff(0x000A))

    def globals_stream(sheet_offsets):
        sheets = b"".join(
            biff(0x0085, struct.pack("<IBBBB", off, 0, 0, len(name), 0) + name.encode("latin-1"))
            for (name, _), off in zip(SHEETS, sheet_offsets)
        )
        return (bof(0x0005) + biff(0x0042, struct.pack("<H", 1200)) + window1
                + font * 5 + style_xf * 15 + cell_xf + sheets + biff(0x00FC, sst) + biff(0x000A))

    bodies = [sheet_stream(rows) for _, rows in SHEETS]
    head = len(globals_stream([0] * len(SHEETS)))
    offsets, at = [], head
    for b in bodies:
        offsets.append(at)
        at += len(b)
    workbook = globals_stream(offsets) + b"".join(bodies)
    # Excel itself never writes a Workbook stream small enough for the mini
    # stream; some readers assume it.
    workbook += bytes(max(0, CUTOFF - len(workbook)))
    return compound_file([("Workbook", workbook)])


# --- Word 97 via textutil -----------------------------------------------------

REPORT_HTML = """<html><body>
<h1>Goblin Hoard Inventory</h1>
<p>The hoard grew by <b>212 items</b> this season, an <i>alarming</i> rate.
See the <a href="https://example.com/hoard">full ledger</a> for details.</p>
<h2>Holdings by Cave</h2>
<table border="1">
<tr><th>Cave</th><th>Items</th><th>Shiny?</th></tr>
<tr><td>North Cave</td><td>1,204</td><td>Yes</td></tr>
<tr><td>Bog Hollow</td><td></td><td>No</td></tr>
</table>
<p>Unsorted finds:</p>
<ul><li>Socks (unpaired)</li><li>Spoons</li></ul>
<ol><li>Count the spoons</li><li>Hide the spoons</li></ol>
</body></html>
"""


def word_97(path):
    if not shutil.which("textutil"):
        print(f"skipped {os.path.basename(path)}: needs macOS textutil")
        return
    with tempfile.TemporaryDirectory() as tmp:
        html = os.path.join(tmp, "report.html")
        with open(html, "w", encoding="utf-8") as f:
            f.write(REPORT_HTML)
        subprocess.run(["textutil", "-convert", "doc", html, "-output", path], check=True)
    print(f"wrote {os.path.basename(path)}")


if __name__ == "__main__":
    for name, build in [("legacy_deck.ppt", powerpoint_97), ("legacy_ledger.xls", excel_97)]:
        with open(os.path.join(HERE, name), "wb") as f:
            f.write(build())
        print(f"wrote {name}")
    word_97(os.path.join(HERE, "legacy_report.doc"))
