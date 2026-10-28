"""Create Windows icon sizes using Pillow, matching the checked-in logo.svg."""
from pathlib import Path
from PIL import Image, ImageDraw
ROOT=Path(__file__).resolve().parents[1]
DEST=ROOT/"src-tauri"/"icons"
def paint(size):
 s=4; im=Image.new("RGBA",(256*s,256*s));d=ImageDraw.Draw(im)
 def coords(v):return tuple(int(a*s) for a in v)
 d.rounded_rectangle(coords((0,0,255,255)),radius=55*s,fill="#141d2c")
 d.polygon([(29*s,92*s),(128*s,40*s),(227*s,92*s),(128*s,125*s)],fill="#f54852")
 d.polygon([(128*s,40*s),(227*s,92*s),(128*s,125*s)],fill="#22dac9")
 d.ellipse(coords((45,63,211,229)),fill="#ed3046")
 d.pieslice(coords((45,63,211,229)),270,90,fill="#20d9c9")
 d.ellipse(coords((105,95,196,206)),fill="#effff9")
 for x,y in ((73,137),(83,169),(109,190)):d.ellipse(coords((x-11,y-11,x+11,y+11)),fill="#8d2333")
 for x in (144,174):d.rounded_rectangle(coords((x,129,x+13,168)),radius=6*s,fill="#008e80")
 d.line([(226*s,98*s),(226*s,140*s)],fill="#1af4d0",width=9*s)
 d.ellipse(coords((216,139,236,159)),fill="#1af4d0")
 return im.resize((size,size),Image.Resampling.LANCZOS)
def main():
 DEST.mkdir(parents=True,exist_ok=True)
 for size,name in ((32,"32x32.png"),(128,"128x128.png"),(256,"128x128@2x.png")):paint(size).save(DEST/name)
 paint(256).save(DEST/"icon.ico",format="ICO",sizes=[(16,16),(32,32),(48,48),(64,64),(128,128)])
if __name__=="__main__":main()
