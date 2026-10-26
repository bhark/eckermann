# dmgbuild settings, run through dmg.sh
files = [defines["app"]]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["Eckermann.app"]
icon = "icons/icon.icns"
# the @2x png beside it makes a retina tiff
background = f"installer/art/dmg-{defines['look']}.png"
format = "ULFO"
window_rect = ((200, 120), (640, 400))
text_size = 13
icon_locations = {"Eckermann.app": (176, 196), "Applications": (464, 196)}
