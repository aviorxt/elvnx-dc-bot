from PIL import Image,ImageDraw,ImageFont,ImageChops
from pathlib import Path
root=Path(__file__).resolve().parents[1]
names=['check','pause','clock','unlock','shield','volume_up','review','repeat','mail','headphones','equalizer','typing','trophy','rocket','search','calendar']
frames=[];font=ImageFont.truetype(str(root/'assets/fonts/SpaceGrotesk.ttf'),16)
def at_time(im,stamp):
    cumulative=0
    for index in range(im.n_frames):
        im.seek(index);cumulative+=im.info.get('duration',80)
        if stamp<cumulative:return im.convert('RGBA')
    return im.convert('RGBA')
for i in range(24):
    sheet=Image.new('RGB',(560,648),'#0b0c0f');d=ImageDraw.Draw(sheet)
    for n,name in enumerate(names):
        im=Image.open(root/f'assets/emojis/elvnx_live_{name}.gif');tile=at_time(im,i*80);x=n%4*140+6;y=n//4*162+2
        sheet.paste(tile,(x,y),tile);d.text((x+4,y+132),name,font=font,fill='#eeeeee')
    frames.append(sheet)
frames[0].save(root/'previews/emojis-animated.gif',save_all=True,append_images=frames[1:],duration=80,loop=0)
frames[6].save(root/'previews/animated-contact.png')
for path in (root/'assets/emojis').glob('elvnx_live_*.gif'):
    im=Image.open(path);assert im.n_frames>1 and path.stat().st_size<256*1024
    first=at_time(im,0).convert('RGB');changed=False
    for t in range(80,1920,80):
        delta=ImageChops.difference(first,at_time(im,t).convert('RGB'));box=delta.getbbox()
        if box:
            changed=True
            assert box[0]>=10 and box[1]>=10 and box[2]<=118 and box[3]<=118,(path.name,box)
    assert changed,path.name
print('40 animated glyphs validated: moving symbols, stationary badge edges, upload sizes within limits.')

