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
            "A single-binary orchestrator for teams\n"
            "that left Kubernetes on purpose"
        ),
        "is_title_card": True,
        "duration": 6,
    },
    {
        "annotation": (
            "One TOML file defines your entire stack.\n"
            "Processes, containers, databases, workers, cron jobs."
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
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "worker"', (220, 220, 220)),
            ('run = "./api process-jobs"', (220, 220, 220)),
            ('after = ["db", "redis"]', (220, 220, 220)),
            ("", None),
            ("[[service]]", (150, 200, 255)),
            ('name = "reports"', (220, 220, 220)),
            ('run = "./api generate-reports"', (220, 220, 220)),
            ('schedule = "0 2 * * *"', (220, 220, 220)),
            ('after = ["db"]', (220, 220, 220)),
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
        "duration": 10,
    },
    {
        "annotation": (
            "Live dashboard shows real service state.\n"
            "Status updates on crash, restart, recovery."
        ),
        "action": "dashboard_healthy",
        "duration": 8,
    },
    {
        "annotation": (
            "When a service crashes, the dashboard reflects it immediately.\n"
            "Restart count and current status are always accurate."
        ),
        "action": "dashboard_crash",
        "duration": 8,
    },
    {
        "annotation": (
            "Graceful shutdown sends SIGTERM, waits 10 seconds, then SIGKILL.\n"
            "Services stop in reverse dependency order."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("^C", (255, 200, 100)),
            ("", None),
            ("shutting down...", (220, 220, 220)),
            ("  stopped reports", (180, 180, 180)),
            ("  stopped worker", (180, 180, 180)),
            ("  stopped api", (180, 180, 180)),
            ("  stopped redis", (180, 180, 180)),
            ("  stopped db", (180, 180, 180)),
            ("done.", (74, 222, 128)),
        ],
        "duration": 8,
    },
    {
        "annotation": (
            "baton add scaffolds common services in one command.\n"
            "Postgres, Redis, MySQL, Mongo, RabbitMQ, NATS, workers, cron, static, SPA."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ baton add postgres", (150, 200, 255)),
            ("added 'postgres' to baton.toml", (74, 222, 128)),
            ("", None),
            ("$ baton add redis", (150, 200, 255)),
            ("added 'redis' to baton.toml", (74, 222, 128)),
            ("", None),
            ("$ baton add worker --run './app process-jobs'", (150, 200, 255)),
            ("added 'worker' to baton.toml", (74, 222, 128)),
            ("", None),
            ("$ baton add cron --name nightly --run './app cleanup' --schedule '0 3 * * *'", (150, 200, 255)),
            ("added 'nightly' to baton.toml", (74, 222, 128)),
            ("", None),
            ("$ baton add spa --name frontend", (150, 200, 255)),
            ("added 'frontend' to baton.toml", (74, 222, 128)),
        ],
        "duration": 10,
    },
    {
        "annotation": (
            "68 tests validate config parsing, dependency ordering,\n"
            "service discovery, and env var injection."
        ),
        "is_terminal": True,
        "terminal_lines": [
            ("$ cargo test", (150, 200, 255)),
            ("", None),
            ("running 8 tests (lib)", (180, 180, 180)),
            ("test env_file::tests::simple_vars ... ok", (74, 222, 128)),
            ("test env_file::tests::quoted_values ... ok", (74, 222, 128)),
            ("test env_file::tests::comments_and_blanks ... ok", (74, 222, 128)),
            ("test health::tests::port_not_listening_fails ... ok", (74, 222, 128)),
            ("", None),
            ("running 20 tests (add_tests)", (180, 180, 180)),
            ("test add_postgres ... ok", (74, 222, 128)),
            ("test add_redis ... ok", (74, 222, 128)),
            ("test add_worker_with_custom_command ... ok", (74, 222, 128)),
            ("test add_cron_with_schedule ... ok", (74, 222, 128)),
            ("test add_duplicate_fails ... ok", (74, 222, 128)),
            ("", None),
            ("running 18 tests (config_tests)", (180, 180, 180)),
            ("test duplicate_service_names_rejected ... ok", (74, 222, 128)),
            ("test valid_dependency_chain ... ok", (74, 222, 128)),
            ("test deep_dependency_chain ... ok", (74, 222, 128)),
            ("test many_services_stress ... ok", (74, 222, 128)),
            ("", None),
            ("running 17 tests (runner_tests)", (180, 180, 180)),
            ("test toposort_complex_graph ... ok", (74, 222, 128)),
            ("test env_var_injection_postgres ... ok", (74, 222, 128)),
            ("test env_var_injection_postgres_custom_password ... ok", (74, 222, 128)),
            ("test default_ports_known_images ... ok", (74, 222, 128)),
            ("", None),
            ("test result: ok. 68 passed; 0 failed", (74, 222, 128)),
        ],
        "duration": 10,
    },
    {
        "title": "Baton",
        "subtitle": (
            "Single binary. One TOML file.\n"
            "Processes + containers + cron in one place.\n"
            "SIGTERM graceful shutdown. Live dashboard.\n"
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
