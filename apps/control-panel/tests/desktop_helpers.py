"""Real GTK rendering and isolated real-adapter configuration for UI checks."""
import ctypes
import json
from pathlib import Path


def real_configuration(root: Path, directory: Path):
    source=root/'apps/adapter-host/config/multiple-adapters'
    for file in source.glob('*.json'):
        (directory/file.name).write_bytes(file.read_bytes())
    path=directory/'host.json'
    config=json.loads(path.read_text())
    config['database']='state/core.sqlite'
    config['schemas']=[str((source/value).resolve()) for value in config['schemas']]
    for adapter in config['adapters']:
        adapter['program']=str((source/adapter['program']).resolve())
        adapter['timeout_ms']=15000
        for arg in adapter.get('args',[]):
            if isinstance(arg,dict):
                arg['path']='state/'+Path(arg['path']).name
    path.write_text(json.dumps(config))
    for side in ['left','right']:
        path=directory/f'output-{side}.json'
        profile=json.loads(path.read_text());profile['directory']='reports-'+side
        path.write_text(json.dumps(profile))
    return directory/'host.json'


def render_window(window, target: Path):
    """gtk_widget_draw uses the real allocated layout, styles and widgets."""
    import time
    from gi.repository import Gtk
    until=time.monotonic()+.25
    while time.monotonic()<until:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        time.sleep(.01)
    cairo=ctypes.CDLL('libcairo.so.2');gtk=ctypes.CDLL('libgtk-3.so.0')
    cairo.cairo_image_surface_create.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_int]
    cairo.cairo_image_surface_create.restype=ctypes.c_void_p
    cairo.cairo_create.argtypes=[ctypes.c_void_p];cairo.cairo_create.restype=ctypes.c_void_p
    cairo.cairo_surface_write_to_png.argtypes=[ctypes.c_void_p,ctypes.c_char_p]
    cairo.cairo_surface_write_to_png.restype=ctypes.c_int
    cairo.cairo_destroy.argtypes=[ctypes.c_void_p];cairo.cairo_surface_destroy.argtypes=[ctypes.c_void_p]
    gtk.gtk_widget_draw.argtypes=[ctypes.c_void_p,ctypes.c_void_p]
    capsule=ctypes.pythonapi.PyCapsule_GetPointer
    capsule.argtypes=[ctypes.py_object,ctypes.c_char_p];capsule.restype=ctypes.c_void_p
    target.parent.mkdir(parents=True,exist_ok=True)
    surface=cairo.cairo_image_surface_create(0,window.get_allocated_width(),window.get_allocated_height())
    context=cairo.cairo_create(surface)
    try:
        gtk.gtk_widget_draw(capsule(window.__gpointer__,None),context)
        if cairo.cairo_surface_write_to_png(surface,str(target).encode()):
            raise RuntimeError('GTK screenshot failed')
    finally:
        cairo.cairo_destroy(context);cairo.cairo_surface_destroy(surface)
