from __future__ import annotations

import json
import subprocess
import threading
import time
from http.server import HTTPServer, SimpleHTTPRequestHandler
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
from playwright.sync_api import sync_playwright

FRAMES_DIR = Path("demo_frames")
OUTPUT = Path("assets/demo.mp4")
DASHBOARD_HTML = Path("src/dashboard.html")
WIDTH = 1280
HEIGHT = 800
FPS = 1

MOCK_STATUS = {
    "domain": "myapp.com",
    "services": [
        {"name": "db", "kind": "container", "detail": "postgres:16", "port": 5432, "schedule": None, "status": "running", "restarts": 0},
        {"name": "redis", "kind": "container", "detail": "redis:7", "port": 6379, "schedule": None, "status": "running", "restarts": 0},
        {"name": "api", "kind": "process", "detail": "./api serve", "port": 4000, "schedule": None, "status": "running", "restarts": 0},
        {"name": "worker", "kind": "process", "detail": "./api process-jobs", "port": None, "schedule": None, "status": "running", "restarts": 0},
        {"name": "reports", "kind": "cron", "detail": "./api generate-reports", "port": None, "schedule": "0 2 * * *", "status": "scheduled", "restarts": 0},
    ],
}

MOCK_STATUS_CRASH = {
    "domain": "myapp.com",
    "services": [
        {"name": "db", "kind": "container", "detail": "postgres:16", "port": 5432, "schedule": None, "status": "running", "restarts": 0},
        {"name": "redis", "kind": "container", "detail": "redis:7", "port": 6379, "schedule": None, "status": "running", "restarts": 0},
        {"name": "api", "kind": "process", "detail": "./api serve", "port": 4000, "schedule": None, "status": "restarting", "restarts": 2},
        {"name": "worker", "kind": "process", "detail": "./api process-jobs", "port": None, "schedule": None, "status": "running", "restarts": 0},
        {"name": "reports", "kind": "cron", "detail": "./api generate-reports", "port": None, "schedule": "0 2 * * *", "status": "scheduled", "restarts": 0},
    ],
}

current_status = MOCK_STATUS


class MockHandler(SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/api/status":
            data = json.dumps(current_status).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", len(data))
            self.end_headers()
            self.wfile.write(data)
        elif self.path == "/":
            html = DASHBOARD_HTML.read_text()
            data = html.encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", len(data))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):
        pass


SCENES = [
    {
        "title": "Baton",
        "subtitle": (
            "A single-binary deploy engine\n"
            "for single-node production systems"
        ),
        "is_title_card": True,
        "duration": 6,
    },
    {
        "annotation": (
            "One TOML file defines your entire stack.\n"
            "backup and migrate fields enable safe deploys."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ cat baton.toml", (150, 200, 255)),
            ("", None),
            ("[app]", (220, 220, 220)),
            ('name = "myapp"', (220, 220, 220)),
            ('domain = "myapp.com"', (220, 220, 220)),
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "db"', (220, 220, 220)),
            ('image = "postgres:16"', (220, 220, 220)),
            ('volume = "pg_data"', (220, 220, 220)),
            ('backup = "pg_dump"', (74, 222, 128)),
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "redis"', (220, 220, 220)),
            ('image = "redis:7"', (220, 220, 220)),
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "api"', (220, 220, 220)),
            ('run = "./api serve"', (220, 220, 220)),
            ("port = 4000", (220, 220, 220)),
            ('health = "/health"', (220, 220, 220)),
            ('after = ["db", "redis"]', (220, 220, 220)),
            ('migrate = "./api migrate"', (74, 222, 128)),
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "worker"', (220, 220, 220)),
            ('run = "./api process-jobs"', (220, 220, 220)),
            ('after = ["db", "redis"]', (220, 220, 220)),
        ],
        "duration": 10,
    },
    {
        "annotation": (
            "baton up starts everything in dependency order.\n"
            "Containers, processes, health checks, service discovery."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton up --ui", (150, 200, 255)),
            ("loaded 3 vars from .env", (180, 180, 180)),
            ("", None),
            ("starting myapp...", (220, 220, 220)),
            ("", None),
            ("  [ok] db      postgres:16 on :5432", (74, 222, 128)),
            ("  [ok] redis   redis:7 on :6379", (74, 222, 128)),
            ("  [ok] api     ./api serve on :4000", (74, 222, 128)),
            ("  [ok] worker  ./api process-jobs running", (74, 222, 128)),
            ("  [ok] reports ./api generate-reports scheduled (0 2 * * *)", (74, 222, 128)),
            ("  [ui] dashboard at http://localhost:9500", (180, 180, 180)),
            ("", None),
            ("all services running. ctrl+c to stop.", (220, 220, 220)),
        ],
        "duration": 8,
    },
    {
        "annotation": (
            "baton deploy snapshots your database before every deploy.\n"
            "Migrations run in dependency order. Health checks gate the rollout."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton deploy", (150, 200, 255)),
            ("loaded 3 vars from .env", (180, 180, 180)),
            ("deploying myapp...", (220, 220, 220)),
            ("", None),
            ("  snapshotting stateful services...", (220, 220, 220)),
            ("    [ok] db (pg_dump)", (74, 222, 128)),
            ("    [ok] redis (redis)", (74, 222, 128)),
            ("", None),
            ("  running migrations...", (220, 220, 220)),
            ("    api ... ok", (74, 222, 128)),
            ("", None),
            ("  restarting services...", (220, 220, 220)),
            ("    [ok] api (container)", (74, 222, 128)),
            ("    [ok] worker (signalled)", (74, 222, 128)),
            ("", None),
            ("  checking health...", (220, 220, 220)),
            ("    api :4000/health ... ok", (74, 222, 128)),
            ("", None),
            ("deploy complete.", (74, 222, 128)),
        ],
        "duration": 12,
    },
    {
        "annotation": (
            "When a deploy fails, baton rolls back automatically.\n"
            "Database restored from snapshot. No manual intervention."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton deploy", (150, 200, 255)),
            ("loaded 3 vars from .env", (180, 180, 180)),
            ("deploying myapp...", (220, 220, 220)),
            ("", None),
            ("  snapshotting stateful services...", (220, 220, 220)),
            ("    [ok] db (pg_dump)", (74, 222, 128)),
            ("", None),
            ("  running migrations...", (220, 220, 220)),
            ("    api ... ok", (74, 222, 128)),
            ("", None),
            ("  restarting services...", (220, 220, 220)),
            ("    [ok] api (container)", (74, 222, 128)),
            ("", None),
            ("  checking health...", (220, 220, 220)),
            ("    api :4000/health ... FAILED", (255, 100, 100)),
            ("", None),
            ("  health check failed, restoring snapshot 20260329-143000...", (255, 200, 100)),
            ("    [ok] db restored (pg_dump)", (255, 200, 100)),
            ("", None),
            ("Error: health check failed for api, rolled back to snapshot 20260329-143000", (255, 100, 100)),
        ],
        "duration": 12,
    },
    {
        "annotation": (
            "baton history shows the full deploy timeline.\n"
            "Every snapshot, migration, health check, and rollback recorded."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton history", (150, 200, 255)),
            ("deploy history", (220, 220, 220)),
            ("", None),
            ("  20260329-140000 [ok] 2026-03-29T14:00:00Z", (74, 222, 128)),
            ("    deploy start deploy started", (180, 180, 180)),
            ("    snapshot snapshot 20260329-140000", (180, 180, 180)),
            ("    migrate api migration succeeded", (180, 180, 180)),
            ("    restart api restarted", (180, 180, 180)),
            ("    health pass api healthy", (180, 180, 180)),
            ("    deploy complete Success", (74, 222, 128)),
            ("", None),
            ("  20260329-143000 [ROLLED BACK] 2026-03-29T14:30:00Z", (255, 200, 100)),
            ("    deploy start deploy started", (180, 180, 180)),
            ("    snapshot snapshot 20260329-143000", (180, 180, 180)),
            ("    migrate api migration succeeded", (180, 180, 180)),
            ("    restart api restarted", (180, 180, 180)),
            ("    health failed api connection refused", (255, 100, 100)),
            ("    rollback restored snapshot 20260329-143000", (255, 200, 100)),
            ("    deploy complete RolledBack", (255, 200, 100)),
        ],
        "duration": 10,
    },
    {
        "annotation": (
            "Live dashboard shows real service state.\n"
            "Status updates on crash, restart, recovery."
        ),
        "action": "dashboard_healthy",
        "duration": 6,
    },
    {
        "annotation": (
            "When a service crashes, the dashboard reflects it immediately.\n"
            "Restart count and current status are always accurate."
        ),
        "action": "dashboard_crash",
        "duration": 6,
    },
    {
        "annotation": (
            "Manual snapshot and rollback available any time.\n"
            "No deploy required."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton snapshot", (150, 200, 255)),
            ("taking snapshot...", (220, 220, 220)),
            ("", None),
            ("  [ok] db (pg_dump)", (74, 222, 128)),
            ("  [ok] redis (redis)", (74, 222, 128)),
            ("", None),
            ("snapshot 20260329-150000 saved.", (74, 222, 128)),
            ("", None),
            ("", None),
            ("$ baton rollback", (150, 200, 255)),
            ("restoring snapshot 20260329-150000...", (220, 220, 220)),
            ("", None),
            ("  [ok] db restored (pg_dump)", (74, 222, 128)),
            ("  [ok] redis restored (redis)", (74, 222, 128)),
            ("", None),
            ("rollback complete.", (74, 222, 128)),
        ],
        "duration": 8,
    },
    {
        "annotation": (
            "82 tests validate config, dependency ordering,\n"
            "snapshots, migrations, deploy lifecycle, and rollback."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ cargo test", (150, 200, 255)),
            ("", None),
            ("running 8 tests (lib)", (180, 180, 180)),
            ("test env_file::tests::simple_vars ... ok", (74, 222, 128)),
            ("test env_file::tests::quoted_values ... ok", (74, 222, 128)),
            ("test health::tests::port_not_listening_fails ... ok", (74, 222, 128)),
            ("", None),
            ("running 20 tests (add_tests)", (180, 180, 180)),
            ("test add_postgres ... ok", (74, 222, 128)),
            ("test add_redis ... ok", (74, 222, 128)),
            ("test add_cron_with_schedule ... ok", (74, 222, 128)),
            ("", None),
            ("running 18 tests (config_tests)", (180, 180, 180)),
            ("test valid_dependency_chain ... ok", (74, 222, 128)),
            ("test deep_dependency_chain ... ok", (74, 222, 128)),
            ("", None),
            ("running 14 tests (deploy_tests)", (180, 180, 180)),
            ("test postgres_has_implicit_backup ... ok", (74, 222, 128)),
            ("test deploy_recorder_tracks_events ... ok", (74, 222, 128)),
            ("test migrations_run_in_topo_order ... ok", (74, 222, 128)),
            ("test full_deploy_config_parses ... ok", (74, 222, 128)),
            ("", None),
            ("running 17 tests (runner_tests)", (180, 180, 180)),
            ("test toposort_complex_graph ... ok", (74, 222, 128)),
            ("test env_var_injection_postgres ... ok", (74, 222, 128)),
            ("", None),
            ("test result: ok. 82 passed; 0 failed", (74, 222, 128)),
        ],
        "duration": 8,
    },
    {
        "title": "Baton",
        "subtitle": (
            "Snapshot. Migrate. Health gate. Auto-rollback.\n"
            "One binary. One TOML file. No cluster required.\n"
            "github.com/michaelmillar/baton"
        ),
        "is_title_card": True,
        "duration": 8,
    },
]


def get_font(size: int) -> ImageFont.FreeTypeFont:
    font_paths = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-Bold.ttf",
    ]
    for fp in font_paths:
        if Path(fp).exists():
            return ImageFont.truetype(fp, size)
    return ImageFont.load_default()


def get_font_regular(size: int) -> ImageFont.FreeTypeFont:
    font_paths = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-Regular.ttf",
    ]
    for fp in font_paths:
        if Path(fp).exists():
            return ImageFont.truetype(fp, size)
    return ImageFont.load_default()


def get_font_mono(size: int) -> ImageFont.FreeTypeFont:
    font_paths = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/UbuntuMono-Regular.ttf",
    ]
    for fp in font_paths:
        if Path(fp).exists():
            return ImageFont.truetype(fp, size)
    return ImageFont.load_default()


def create_title_card(title: str, subtitle: str, frame_path: Path) -> None:
    img = Image.new("RGB", (WIDTH, HEIGHT), color=(17, 17, 17))
    draw = ImageDraw.Draw(img)

    title_font = get_font(64)
    sub_font = get_font_regular(26)

    title_bbox = draw.textbbox((0, 0), title, font=title_font)
    title_w = title_bbox[2] - title_bbox[0]
    draw.text(
        ((WIDTH - title_w) // 2, HEIGHT // 2 - 100),
        title,
        fill=(255, 255, 255),
        font=title_font,
    )

    for i, line in enumerate(subtitle.split("\n")):
        line_bbox = draw.textbbox((0, 0), line, font=sub_font)
        line_w = line_bbox[2] - line_bbox[0]
        draw.text(
            ((WIDTH - line_w) // 2, HEIGHT // 2 + i * 42),
            line,
            fill=(170, 170, 170),
            font=sub_font,
        )

    img.save(frame_path)


def create_terminal_frame(lines: list, frame_path: Path) -> None:
    img = Image.new("RGB", (WIDTH, HEIGHT), color=(17, 17, 17))
    draw = ImageDraw.Draw(img)
    font = get_font_mono(18)

    y = 50
    for text, color in lines:
        if color is None:
            y += 10
            continue
        draw.text((50, y), text, fill=color, font=font)
        y += 24

    img.save(frame_path)


def add_annotation(screenshot_path: Path, annotation: str, output_path: Path) -> None:
    img = Image.open(screenshot_path)
    img = img.resize((WIDTH, HEIGHT), Image.LANCZOS)

    overlay = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)

    bar_height = 80
    draw.rectangle(
        [(0, HEIGHT - bar_height), (WIDTH, HEIGHT)],
        fill=(17, 17, 17, 230),
    )

    font = get_font_regular(22)
    lines = annotation.split("\n")
    y = HEIGHT - bar_height + 12
    for line in lines:
        line_bbox = draw.textbbox((0, 0), line, font=font)
        line_w = line_bbox[2] - line_bbox[0]
        draw.text(
            ((WIDTH - line_w) // 2, y),
            line,
            fill=(220, 220, 220, 255),
            font=font,
        )
        y += 30

    img = img.convert("RGBA")
    img = Image.alpha_composite(img, overlay)
    img.convert("RGB").save(output_path)


def run_demo() -> None:
    global current_status

    FRAMES_DIR.mkdir(exist_ok=True)
    OUTPUT.parent.mkdir(exist_ok=True)

    for f in FRAMES_DIR.glob("*.png"):
        f.unlink()

    server = HTTPServer(("127.0.0.1", 19500), MockHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    print("Mock server running on :19500")

    frame_num = 0

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": WIDTH, "height": HEIGHT})

        for scene in SCENES:
            label = scene.get("action", scene.get("title", "terminal"))
            print(f"  Scene {frame_num}: {label}")

            if scene.get("is_title_card"):
                for _ in range(scene["duration"] * FPS):
                    frame_path = FRAMES_DIR / f"frame_{frame_num:04d}.png"
                    create_title_card(scene["title"], scene["subtitle"], frame_path)
                    frame_num += 1
                continue

            if scene.get("is_terminal"):
                terminal_lines = scene.get("terminal_lines", [])
                for _ in range(scene["duration"] * FPS):
                    frame_path = FRAMES_DIR / f"frame_{frame_num:04d}.png"
                    create_terminal_frame(terminal_lines, frame_path)
                    annotation = scene.get("annotation", "")
                    if annotation:
                        add_annotation(frame_path, annotation, frame_path)
                    frame_num += 1
                continue

            action = scene.get("action", "")

            if action == "dashboard_healthy":
                current_status = MOCK_STATUS
            elif action == "dashboard_crash":
                current_status = MOCK_STATUS_CRASH

            page.goto("http://127.0.0.1:19500/", wait_until="networkidle", timeout=10000)
            time.sleep(3)

            screenshot_path = FRAMES_DIR / f"raw_{frame_num:04d}.png"
            page.screenshot(path=str(screenshot_path), full_page=False)

            annotation = scene.get("annotation", "")
            for _ in range(scene["duration"] * FPS):
                frame_path = FRAMES_DIR / f"frame_{frame_num:04d}.png"
                if annotation:
                    add_annotation(screenshot_path, annotation, frame_path)
                else:
                    img = Image.open(screenshot_path)
                    img = img.resize((WIDTH, HEIGHT), Image.LANCZOS)
                    img.save(frame_path)
                frame_num += 1

            screenshot_path.unlink(missing_ok=True)

        browser.close()

    server.shutdown()

    print(f"Generated {frame_num} frames. Encoding video...")

    cmd = [
        "ffmpeg", "-y",
        "-framerate", str(FPS),
        "-i", str(FRAMES_DIR / "frame_%04d.png"),
        "-c:v", "libx264",
        "-pix_fmt", "yuv420p",
        "-r", "30",
        "-preset", "medium",
        "-crf", "23",
        str(OUTPUT),
    ]
    subprocess.run(cmd, check=True)

    for f in FRAMES_DIR.glob("*.png"):
        f.unlink()
    FRAMES_DIR.rmdir()

    print(f"Done. Video saved to {OUTPUT}")


if __name__ == "__main__":
    run_demo()
