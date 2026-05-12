import os
import tkinter as tk
from PIL import Image, ImageTk

from config import EQ_PATH

TRANSPARENT_COLOR = "#FF00FF"


class Overlay:
    def __init__(self):
        self.root = tk.Tk()
        self.root.overrideredirect(True)
        self.root.attributes("-topmost", True)
        self.root.attributes("-transparentcolor", TRANSPARENT_COLOR)
        self.root.configure(bg=TRANSPARENT_COLOR)

        self.width = 200
        self.height = 150
        self.canvas = tk.Canvas(
            self.root,
            width=self.width,
            height=self.height,
            bg=TRANSPARENT_COLOR,
            highlightthickness=0,
        )
        self.canvas.pack()

        screen_width = self.root.winfo_screenwidth()
        screen_height = self.root.winfo_screenheight()
        x = (screen_width - self.width) // 2
        y = screen_height - self.height - 50
        self.root.geometry(f"{self.width}x{self.height}+{x}+{y}")

        self.equalizer_active = False
        self.after_id = None
        self.eq_frames = []
        self.eq_durations = []
        self.eq_frame_index = 0
        self.eq_image_id = None

        self._load_equalizer_gif()

    def _load_equalizer_gif(self):
        if not os.path.exists(EQ_PATH):
            return
        try:
            img = Image.open(EQ_PATH)
            while True:
                frame = img.convert("RGBA")
                # Убираем белый/светлый фон, заменяя его на цвет прозрачности окна
                data = list(frame.getdata())
                new_data = []
                for r, g, b, a in data:
                    if r > 240 and g > 240 and b > 240:
                        new_data.append((255, 0, 255, 255))  # TRANSPARENT_COLOR
                    else:
                        new_data.append((r, g, b, a))
                frame.putdata(new_data)

                photo = ImageTk.PhotoImage(frame)
                self.eq_frames.append(photo)
                self.eq_durations.append(img.info.get("duration", 40))
                img.seek(len(self.eq_frames))
        except EOFError:
            pass

    def run(self):
        self.hide()
        self.root.mainloop()

    def hide(self):
        self.root.withdraw()

    def show(self):
        self.root.deiconify()
        self.root.lift()

    def schedule(self, func):
        if self.root.winfo_exists():
            self.root.after(0, func)

    def after(self, ms, func):
        if self.root.winfo_exists():
            self.root.after(ms, func)

    def clear(self):
        self.equalizer_active = False
        if self.after_id is not None:
            try:
                self.root.after_cancel(self.after_id)
            except Exception:
                pass
            self.after_id = None
        self.canvas.delete("all")
        self.eq_frame_index = 0
        self.eq_image_id = None

    def animate_equalizer(self):
        self.clear()
        self.show()
        self.equalizer_active = True

        if self.eq_frames:
            x = self.width // 2
            y = self.height // 2
            self.eq_image_id = self.canvas.create_image(x, y, image=self.eq_frames[0])
            self._update_gif()
        else:
            # fallback: простые полоски
            self._draw_fallback_bars()

    def _update_gif(self):
        if not self.equalizer_active or not self.eq_frames:
            return
        self.eq_frame_index = (self.eq_frame_index + 1) % len(self.eq_frames)
        self.canvas.itemconfig(
            self.eq_image_id, image=self.eq_frames[self.eq_frame_index]
        )
        delay = self.eq_durations[self.eq_frame_index] if self.eq_durations else 40
        self.after_id = self.root.after(delay, self._update_gif)

    def _draw_fallback_bars(self):
        import random
        num_bars = 7
        bar_w = 20
        gap = 8
        total = num_bars * bar_w + (num_bars - 1) * gap
        sx = (self.width - total) // 2
        by = self.height - 20
        bars = []
        for i in range(num_bars):
            x1 = sx + i * (bar_w + gap)
            x2 = x1 + bar_w
            b = self.canvas.create_rectangle(x1, by, x2, by, fill="#00D4FF", outline="")
            bars.append(b)

        def anim():
            if not self.equalizer_active:
                return
            for b in bars:
                h = random.randint(10, 60)
                x1, _, x2, _ = self.canvas.coords(b)
                self.canvas.coords(b, x1, by - h, x2, by)
            self.after_id = self.root.after(100, anim)

        anim()

    def show_success(self):
        self.clear()
        self.show()
        self.canvas.create_line(
            80, 75, 100, 95, fill="#00FF00", width=10, capstyle=tk.ROUND
        )
        self.canvas.create_line(
            100, 95, 140, 55, fill="#00FF00", width=10, capstyle=tk.ROUND
        )
        self.after_id = self.root.after(2000, self.hide)

    def show_error(self):
        self.clear()
        self.show()
        self.canvas.create_line(
            80, 55, 140, 95, fill="#FF0000", width=10, capstyle=tk.ROUND
        )
        self.canvas.create_line(
            140, 55, 80, 95, fill="#FF0000", width=10, capstyle=tk.ROUND
        )
        self.after_id = self.root.after(2000, self.hide)
