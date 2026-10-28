import numpy as np
from PIL import Image, ImageFilter
rng=np.random.default_rng(11)
def noise(h,w,f,stretch=(1,1)):
    g=rng.random((max(2,int(f*stretch[0])),max(2,int(f*stretch[1]))))
    return np.asarray(Image.fromarray((g*255).astype(np.uint8)).resize((w,h),Image.BICUBIC),float)/255
def fbm(h,w,o=6,b=3,p=0.55,st=(1,1)):
    out=0;a=1;t=0
    for i in range(o): out=out+a*noise(h,w,b*2**i,st); t+=a; a*=p
    return out/t
W,H=2880,3520
# --- venetian plaster, charcoal ---
base=fbm(H//2,W//2,7,3,0.58); base=np.asarray(Image.fromarray((base*255).astype(np.uint8)).resize((W,H),Image.BICUBIC),float)/255
v=(base-base.min())/(base.max()-base.min())
lum=0.105+0.10*v**1.4
strokes=np.zeros((H,W))
for i in range(520):
    sw=int(rng.integers(260,900)); sh=int(sw*rng.uniform(0.12,0.28))
    patch=noise(sh,sw,6,(0.4,3))*0.6+0.4
    yy,xx=np.mgrid[0:sh,0:sw]; m=np.clip(1-(((xx-sw/2)/(sw/2))**2+((yy-sh/2)/(sh/2))**2),0,1)**1.6
    sign=rng.choice([-1,1])*rng.uniform(0.012,0.035)
    layer=Image.fromarray(((patch*m)*255).astype(np.uint8)).rotate(rng.uniform(-35,35),expand=True,resample=Image.BICUBIC)
    L=np.asarray(layer,float)/255
    y0=int(rng.integers(-L.shape[0]//2,H)); x0=int(rng.integers(-L.shape[1]//2,W))
    ys=slice(max(0,y0),min(H,y0+L.shape[0])); xs=slice(max(0,x0),min(W,x0+L.shape[1]))
    strokes[ys,xs]+=sign*L[ys.start-y0:ys.stop-y0, xs.start-x0:xs.stop-x0]
lum=lum+strokes
lum+= (rng.random((H,W))-0.5)*0.012
lum=np.asarray(Image.fromarray(np.clip(lum*255*2,0,255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.6)),float)/255/2
warm=np.stack([lum*1.02,lum*1.0,lum*0.95],-1)
Image.fromarray(np.clip(warm*255,0,255).astype(np.uint8)).save('plaster@2x.jpg',quality=88)
# --- brushed brass plate with light falloff ---
def brass(w,h,name,ang=0.35):
    rows=rng.random((h,1)); streak=np.repeat(rows,w,1)
    fine=np.asarray(Image.fromarray((rng.random((h,w))*255).astype(np.uint8)).filter(ImageFilter.BoxBlur(0)),float)/255
    fine=np.asarray(Image.fromarray((fine*255).astype(np.uint8)).resize((w//30,h),Image.BILINEAR).resize((w,h),Image.BILINEAR),float)/255
    s=streak*0.35+fine*0.65
    y,x=np.mgrid[0:h,0:w]; d=(x/w*np.cos(ang)+y/h*np.sin(ang))
    spec=np.exp(-((d-0.42)/0.16)**2)*0.9+np.exp(-((d-0.85)/0.1)**2)*0.35
    t=np.clip(0.28+0.5*spec+0.22*(s-0.5),0,1)[...,None]
    dark=np.array([104,78,38]); mid=np.array([176,140,78]); hi=np.array([246,222,168])
    col=np.where(t<0.5, dark+(mid-dark)*(t/0.5), mid+(hi-mid)*((t-0.5)/0.5))
    Image.fromarray(np.clip(col,0,255).astype(np.uint8)).save(name,quality=92)
brass(2000,240,'brass-bar@2x.jpg',0.1)
brass(1000,1300,'brass-plate@2x.jpg',0.6)
# --- cream paper ---
pw,ph=800,1000
f=fbm(ph,pw,5,8,0.5)*0.6+np.asarray(Image.fromarray((rng.random((ph,pw))*255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.5)),float)/255*0.4
c=np.array([243,236,224])+((f-0.5)*14)[...,None]
Image.fromarray(np.clip(c,0,255).astype(np.uint8)).save('paper@2x.jpg',quality=90)
