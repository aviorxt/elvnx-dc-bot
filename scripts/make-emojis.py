"""Render ELVNX's code-native monochrome UI icon set (Pillow, no network)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'assets' / 'emojis'
OUT.mkdir(parents=True, exist_ok=True)
NAMES = ['brand', 'check', 'cross', 'warning', 'play', 'pause', 'next', 'stop',
         'volume_down', 'volume_up', 'queue', 'shuffle', 'repeat', 'shield',
         'lock', 'unlock', 'plus', 'minus', 'link', 'user', 'clock', 'online', 'offline', 'review']
EXTRA_NAMES=['sparkle','equalizer','loading','mail','heart','wave','headphones','microphone','calendar','trophy','rocket','typing','download','upload','search','settings']
NAMES += EXTRA_NAMES
S = 4
def render(name):
    image = Image.new('RGBA', (128*S, 128*S))
    d = ImageDraw.Draw(image)
    def line(points, width=9, fill='#f5f5f5'):
        p = [(int(x*S), int(y*S)) for x,y in points]
        d.line(p, fill=fill, width=width*S, joint='curve')
        r=width*S//2
        for x,y in [p[0],p[-1]]: d.ellipse((x-r,y-r,x+r,y+r),fill=fill)
    def rect(box, radius=8, fill='#f5f5f5'):
        d.rounded_rectangle(tuple(int(v*S) for v in box),radius=radius*S,fill=fill)
    def circle(box, fill=None, outline='#f5f5f5', width=8):
        d.ellipse(tuple(int(v*S) for v in box),fill=fill,outline=outline,width=width*S)
    def poly(points,fill='#f5f5f5'): d.polygon([(int(x*S),int(y*S)) for x,y in points],fill=fill)
    rect((5,5,123,123),26,'#17181c')
    d.rounded_rectangle((5*S,5*S,123*S,123*S),radius=26*S,outline='#6c6e77',width=2*S)
    if name=='brand':
        line([(26,47),(35,39),(35,86)],9);line([(46,47),(55,39),(55,86)],9)
        line([(73,42),(98,86)],10);line([(98,42),(68,86)],10)
    elif name=='check': line([(33,65),(54,86),(95,41)],12)
    elif name=='cross': line([(40,40),(88,88)],11);line([(88,40),(40,88)],11)
    elif name=='warning':
        line([(64,29),(102,96),(26,96),(64,29)],7);line([(64,53),(64,73)],8);circle((60,82,68,90),fill='#fff',width=0)
    elif name=='play': poly([(46,30),(99,64),(46,98)])
    elif name=='pause': rect((38,33,55,95),5);rect((74,33,91,95),5)
    elif name=='next': poly([(32,34),(79,64),(32,94)]);rect((85,34,96,94),4)
    elif name=='stop': rect((36,36,92,92),10)
    elif name.startswith('volume_'):
        poly([(27,52),(42,52),(64,34),(64,94),(42,76),(27,76)])
        line([(79,64),(105,64)],7)
        if name.endswith('up'): line([(92,51),(92,77)],7)
    elif name=='queue':
        for y in [40,64,88]: circle((29,y-4,37,y+4),fill='#fff',width=0);line([(49,y),(97,y)],7)
    elif name=='shuffle':
        line([(28,36),(43,36),(83,91),(101,91)],7);line([(28,91),(43,91),(82,36),(101,36)],7)
        line([(91,26),(102,36),(91,47)],6);line([(91,80),(102,91),(91,102)],6)
    elif name=='repeat':
        line([(28,58),(28,40),(95,40)],7);line([(85,30),(97,40),(85,50)],7)
        line([(100,70),(100,88),(33,88)],7);line([(43,78),(31,88),(43,98)],7)
    elif name=='shield':
        line([(64,25),(97,38),(93,78),(79,96),(64,106),(49,96),(35,78),(31,38),(64,25)],6)
        line([(45,63),(59,77),(83,49)],8)
    elif name in ('lock','unlock'):
        if name=='lock': line([(44,60),(44,42),(50,31),(64,27),(78,31),(84,42),(84,60)],7)
        else: line([(47,60),(47,42),(55,30),(70,27),(84,32),(90,44)],7)
        rect((31,57,97,102),10);rect((61,70,67,88),3,'#17181c')
    elif name=='plus': line([(32,64),(96,64)],10);line([(64,32),(64,96)],10)
    elif name=='minus': line([(32,64),(96,64)],10)
    elif name=='link':
        line([(54,40),(72,25),(90,27),(102,41),(100,58),(83,75)],7)
        line([(45,53),(28,70),(26,87),(39,101),(57,102),(75,86)],7);line([(46,81),(82,45)],8)
    elif name=='user': circle((47,28,81,62),width=7);line([(30,98),(35,82),(49,73),(79,73),(94,82),(98,98)],8)
    elif name=='clock': circle((28,28,100,100),width=7);line([(64,42),(64,65),(81,76)],7)
    elif name=='online': circle((37,37,91,91),fill='#f5f5f5',width=0);circle((26,26,102,102),outline='#71747d',width=3)
    elif name=='offline': circle((37,37,91,91),outline='#a5a7b0',width=8);line([(32,96),(96,32)],6)
    elif name=='review':
        rect((37,27,91,101),6);rect((45,35,83,93),3,'#17181c');line([(48,48),(79,48)],5);line([(48,62),(73,62)],5);line([(48,77),(64,77)],5)
    return image.resize((128,128),Image.Resampling.LANCZOS)

for name in NAMES:
    image=render(name); image.save(OUT/f'elvnx_{name}.png',optimize=True)
sheet=Image.new('RGB',(800,((len(NAMES)+5)//6)*170),'#0b0c0f')
draw=ImageDraw.Draw(sheet); font=ImageFont.truetype(str(ROOT/'assets/fonts/SpaceGrotesk.ttf'),14)
for i,name in enumerate(NAMES):
    x=(i%6)*133+2;y=(i//6)*170+5
    sheet.paste(render(name),(x,y),render(name));draw.text((x+2,y+136),name,font=font,fill='#ededf0')
(ROOT/'previews').mkdir(exist_ok=True);sheet.save(ROOT/'previews/emojis.png')
print(f'Rendered {len(NAMES)} custom emoji assets.')

# Actual glyph animations; the badge border remains completely still.
import math

def motion(name, step):
    frame=Image.new('RGBA',(128*S,128*S))
    d=ImageDraw.Draw(frame)
    d.rounded_rectangle((5*S,5*S,123*S,123*S),radius=26*S,fill='#17181c',outline='#6c6e77',width=2*S)
    ink='#f5f5f5'; phase=step/24; wave=(1-math.cos(phase*2*math.pi))/2
    def line(points,width=8):
        coords=[(round(x*S),round(y*S)) for x,y in points]
        d.line(coords,fill=ink,width=width*S,joint='curve')
        r=width*S/2
        for x,y in (coords[0],coords[-1]):d.ellipse((x-r,y-r,x+r,y+r),fill=ink)
    def poly(points):d.polygon([(round(x*S),round(y*S)) for x,y in points],fill=ink)
    def ellipse(box,width=6,fill=None):d.ellipse(tuple(round(v*S) for v in box),outline=ink,width=width*S,fill=fill)
    def stroke(points,progress,width=9):
        lengths=[math.dist(a,b) for a,b in zip(points,points[1:])];left=sum(lengths)*max(0,min(1,progress))
        for a,b,length in zip(points,points[1:],lengths):
            if left<=0:break
            portion=min(left/length,1);line([a,(a[0]+(b[0]-a[0])*portion,a[1]+(b[1]-a[1])*portion)],width);left-=length
    draw_progress=min(1,(step+1)/12)
    if name in ('equalizer','headphones'):
        if name=='headphones':
            d.arc((29*S,24*S,99*S,102*S),180,360,fill=ink,width=8*S)
            d.rounded_rectangle((26*S,60*S,41*S,96*S),radius=6*S,fill=ink);d.rounded_rectangle((87*S,60*S,102*S,96*S),radius=6*S,fill=ink)
        for i,x in enumerate((49,64,79) if name=='headphones' else (38,64,90)):
            height=12+35*(.5+.5*math.sin(phase*2*math.pi+i*1.4));line([(x,70-height/2),(x,70+height/2)],6 if name=='headphones' else 12)
    elif name in ('loading','typing'):
        for i,x in enumerate((39,64,89)):
            bounce=math.sin(phase*2*math.pi-i*1.6);radius=5+2*max(0,bounce);y=64-8*max(0,bounce)
            ellipse((x-radius,y-radius,x+radius,y+radius),2,ink)
    elif name=='mail':
        d.rounded_rectangle((27*S,49*S,101*S,96*S),radius=5*S,outline=ink,width=6*S)
        top=50-23*wave;poly([(29,50),(64,top),(99,50)])
        d.rectangle((42*S,(55-10*wave)*S,86*S,78*S),fill=ink)
        d.line((49*S,(61-10*wave)*S,78*S,(61-10*wave)*S),fill='#17181c',width=3*S)
        line([(29,52),(64,78),(99,52)],5)
    elif name=='heart':
        points=[];scale=1.5+.25*wave
        for n in range(100):
            a=2*math.pi*n/100;points.append((64+scale*16*math.sin(a)**3,67-scale*(13*math.cos(a)-5*math.cos(2*a)-2*math.cos(3*a)-math.cos(4*a))))
        poly(points)
    elif name=='wave':
        hand=Image.new('RGBA',frame.size);hd=ImageDraw.Draw(hand)
        pts=[(44,92),(36,65),(40,60),(50,72),(44,37),(49,33),(58,64),(55,29),(61,27),(66,61),(68,31),(74,32),(75,65),(81,42),(87,45),(82,79),(70,99),(53,100)]
        hd.polygon([(x*S,y*S) for x,y in pts],fill=ink)
        hand=hand.rotate(12*math.sin(phase*2*math.pi),resample=Image.Resampling.BICUBIC,center=(64*S,89*S));frame.alpha_composite(hand)
    elif name=='microphone':
        d.rounded_rectangle((51*S,25*S,77*S,76*S),radius=12*S,outline=ink,width=7*S)
        d.arc((39*S,41*S,89*S,89*S),0,180,fill=ink,width=6*S);line([(64,89),(64,102)],6);line([(48,103),(80,103)],6)
        for i,y in enumerate((41,52,63)):line([(58,y),(58+8+5*math.sin(phase*2*math.pi+i),y)],3)
    elif name=='calendar':
        d.rounded_rectangle((31*S,33*S,97*S,102*S),radius=6*S,outline=ink,width=6*S)
        line([(32,49),(96,49)],5);line([(46,27),(46,38)],7);line([(82,27),(82,38)],7)
        font=ImageFont.truetype(str(ROOT/'assets/fonts/SpaceGrotesk.ttf'),32*S)
        d.text((45*S,(53-3*wave)*S),'11',font=font,fill=ink)
        fold=7+10*wave;line([(96-fold,101),(96-fold,101-fold),(96,101-fold)],3)
    elif name=='trophy':
        poly([(43,31),(85,31),(81,61),(72,76),(56,76),(47,61)]);line([(64,76),(64,96)],7);line([(46,100),(82,100)],7)
        d.arc((28*S,32*S,55*S,65*S),70,280,fill=ink,width=5*S);d.arc((73*S,32*S,100*S,65*S),-100,110,fill=ink,width=5*S)
        ray=3+4*wave;line([(100-ray,27),(100+ray,27)],3);line([(100,27-ray),(100,27+ray)],3)
    elif name=='rocket':
        dy=-6*wave;poly([(64,24+dy),(79,45+dy),(79,82+dy),(49,82+dy),(49,45+dy)])
        poly([(49,64+dy),(36,89+dy),(49,84+dy)]);poly([(79,64+dy),(92,89+dy),(79,84+dy)])
        d.ellipse((57*S,(49+dy)*S,71*S,(63+dy)*S),fill='#17181c')
        poly([(54,89+dy),(64,102+5*math.sin(phase*4*math.pi)),(74,89+dy)])
    elif name in ('download','upload'):
        offset=9*math.sin(phase*2*math.pi);direction=1 if name=='download' else -1
        start=43 if direction==1 else 80;end=79 if direction==1 else 44
        line([(64,start+offset),(64,end+offset)],9);line([(48,end-16*direction+offset),(64,end+offset),(80,end-16*direction+offset)],8)
        line([(34,89),(34,100),(94,100),(94,89)],6)
    elif name=='search':
        dx=5*math.cos(phase*2*math.pi);dy=4*math.sin(phase*2*math.pi)
        ellipse((34+dx,30+dy,78+dx,74+dy),7);line([(75+dx,72+dy),(97+dx,94+dy)],10)
    elif name=='settings':
        points=[]
        for n in range(48):
            a=n*math.pi/24+phase*2*math.pi;radius=33 if n%4 in (0,1) else 26;points.append((64+radius*math.cos(a),64+radius*math.sin(a)))
        poly(points);d.ellipse((52*S,52*S,76*S,76*S),fill='#17181c')
    elif name=='sparkle':
        for i,(x,y) in enumerate(((51,54),(86,38),(84,87))):
            radius=7+(8 if i==0 else 3)*(.5+.5*math.sin(phase*2*math.pi+i*1.7))
            for a in range(0,360,45):
                angle=math.radians(a);line([(x+3*math.cos(angle),y+3*math.sin(angle)),(x+radius*math.cos(angle),y+radius*math.sin(angle))],3)
    elif name=='check':stroke([(33,65),(54,86),(95,41)],draw_progress,12)
    elif name=='cross':
        stroke([(40,40),(88,88)],min(1,(step+1)/7),10);stroke([(88,40),(40,88)],max(0,(step-6)/7),10)
    elif name in ('play','next'):
        shift=round(8*math.sin(phase*2*math.pi));poly([(43+shift,31),(91+shift,64),(43+shift,97)])
        if name=='next':line([(99,33),(99,95)],7)
    elif name=='pause':
        for i,x in enumerate((47,81)):
            height=36+28*(.5+.5*math.sin(phase*2*math.pi+i*math.pi))
            line([(x,64-height/2),(x,64+height/2)],15)
    elif name in ('volume_up','volume_down'):
        poly([(24,52),(41,52),(62,33),(62,95),(41,76),(24,76)])
        amplitude=3+12*wave
        for i,x in enumerate((77,91)):
            h=amplitude*(1+i*.5)
            d.arc(((x-h/2)*S,(64-h)*S,(x+h/2)*S,(64+h)*S),-75,75,fill=ink,width=5*S)
    elif name=='queue':
        for i,y in enumerate((39,64,89)):
            ellipse((25,y-3,31,y+3),2,ink)
            line([(44,y),(44+max(3,52*min(1,max(0,(step-i*3+1)/8))),y)],7)
    elif name=='clock':
        ellipse((28,28,100,100),7)
        a=phase*2*math.pi-math.pi/2
        line([(64,64),(64+24*math.cos(a),64+24*math.sin(a))],6);line([(64,64),(48,56)],6)
    elif name in ('lock','unlock'):
        gap=8*wave if name=='unlock' else 6*(1-wave)
        d.rounded_rectangle((35*S,56*S,94*S,101*S),radius=9*S,fill=ink)
        d.arc((44*S,(24-gap)*S,84*S,(76-gap)*S),180,360,fill=ink,width=8*S)
        line([(44,49-gap),(44,61)],8)
        line([(84,49-gap),(84,61-gap)],8)
        d.ellipse((60*S,72*S,69*S,81*S),fill='#17181c');d.rectangle((62*S,78*S,67*S,88*S),fill='#17181c')
    elif name=='shield':
        stroke([(64,24),(96,37),(91,75),(78,96),(64,105),(50,96),(37,75),(32,37),(64,24)],1,6)
        stroke([(47,64),(59,77),(81,49)],draw_progress,8)
    elif name=='review':
        d.rounded_rectangle((36*S,25*S,93*S,104*S),radius=6*S,outline=ink,width=6*S)
        for i,y in enumerate((45,61,77)):
            width=38*min(1,max(0,(step-i*4+1)/8));
            if width>0:line([(46,y),(46+width,y)],5)
    elif name=='user':
        nod=4*math.sin(phase*2*math.pi);ellipse((47,26+nod,81,60+nod),6)
        stroke([(30,99),(35,82),(49,74),(79,74),(94,82),(98,99)],1,8)
    elif name in ('online','offline'):
        radius=18+7*wave if name=='online' else 25
        ellipse((64-radius,64-radius,64+radius,64+radius),6,ink if name=='online' else None)
        if name=='offline':stroke([(34,94),(94,34)],draw_progress,7)
    elif name=='warning':
        poly([(64,24),(104,100),(24,100)]);poly_ink=ink
        d.polygon([(64*S,42*S),(88*S,90*S),(40*S,90*S)],fill='#17181c')
        offset=4*math.sin(phase*2*math.pi);line([(64,58+offset),(64,74+offset)],6);ellipse((61,83,67,89),2,ink)
    elif name in ('plus','minus'):
        length=14+36*wave;line([(64-length/2,64),(64+length/2,64)],10)
        if name=='plus':line([(64,64-length/2),(64,64+length/2)],10)
    elif name=='stop':
        radius=22+4*wave;d.rounded_rectangle(((64-radius)*S,(64-radius)*S,(64+radius)*S,(64+radius)*S),radius=7*S,fill=ink)
    else:
        # Transform only the symbol; alpha extraction keeps the fixed badge intact.
        original=render(name).resize((128*S,128*S),Image.Resampling.LANCZOS)
        mask=original.convert('L').point(lambda v:255 if v>185 else 0)
        clip=Image.new('L',original.size);ImageDraw.Draw(clip).rectangle((18*S,18*S,110*S,110*S),fill=255)
        from PIL import ImageChops
        mask=ImageChops.multiply(mask,clip)
        glyph=Image.new('RGBA',original.size,ink);glyph.putalpha(mask)
        if name=='repeat':glyph=glyph.rotate(-360*phase,resample=Image.Resampling.BICUBIC)
        elif name=='shuffle':glyph=glyph.rotate(14*math.sin(phase*2*math.pi),resample=Image.Resampling.BICUBIC)
        elif name=='link':
            # Links physically slide together, then settle.
            left=glyph.crop((0,0,64*S,128*S));right=glyph.crop((64*S,0,128*S,128*S))
            shifted=Image.new('RGBA',glyph.size);dx=round(6*(1-wave)*S)
            shifted.alpha_composite(left,(-dx,0));shifted.alpha_composite(right,(64*S+dx,0));glyph=shifted
        elif name=='brand':
            scale=.88+.12*wave;size=round(128*S*scale);small=glyph.resize((size,size),Image.Resampling.LANCZOS)
            glyph=Image.new('RGBA',original.size);glyph.alpha_composite(small,((128*S-size)//2,(128*S-size)//2))
        frame.alpha_composite(glyph)
    return frame.resize((128,128),Image.Resampling.LANCZOS)

for name in EXTRA_NAMES:
    motion(name,12).save(OUT/f'elvnx_{name}.png',optimize=True)
for name in NAMES:
    frames=[]
    for step in range(24):
        frame=motion(name,step)
        reference=Image.new('P',(1,1));reference.putpalette([v for i in range(256) for v in (i,i,i)])
        palette=frame.convert('RGB').quantize(palette=reference,dither=Image.Dither.NONE)
        palette.paste(255,mask=frame.getchannel('A').point(lambda a:255 if a<128 else 0))
        palette.info['transparency']=255
        frames.append(palette)
    frames[0].save(OUT/f'elvnx_live_{name}.gif',save_all=True,append_images=frames[1:],duration=80,loop=0,transparency=255,disposal=2,optimize=False)
    assert (OUT/f'elvnx_live_{name}.gif').stat().st_size<256*1024
print(f'Rendered {len(NAMES)} animated glyphs with stationary badges.')

sheet=Image.new('RGB',(1064,((len(NAMES)+7)//8)*170),'#0b0c0f');draw=ImageDraw.Draw(sheet);font=ImageFont.truetype(str(ROOT/'assets/fonts/SpaceGrotesk.ttf'),14)
for i,name in enumerate(NAMES):
    im=Image.open(OUT/f'elvnx_{name}.png').convert('RGBA');x=i%8*133+2;y=i//8*170+5;sheet.paste(im,(x,y),im);draw.text((x+2,y+136),name,font=font,fill='#ededed')
sheet.save(ROOT/'previews/emojis.png')
