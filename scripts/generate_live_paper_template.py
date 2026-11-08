"""Rebuild the A4 live-paper PDF and deterministic camera fixtures.

Development dependencies: reportlab, numpy, opencv-python. Runtime uses bundled PDF.
All fixture dimensions are synthetic test scale, not physical measurement evidence.
"""
from pathlib import Path
import cv2
import numpy as np
from reportlab.pdfgen import canvas
from reportlab.lib.units import mm

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'src-tauri/assets'
FIXTURES = ROOT / 'src-tauri/tests/fixtures/live-paper'
CENTERS = [(17,30),(193,30),(193,266),(17,266)]
DICT = cv2.aruco.getPredefinedDictionary(cv2.aruco.DICT_4X4_50)

def main():
    FIXTURES.mkdir(parents=True, exist_ok=True)
    pdf = canvas.Canvas(str(ASSETS / 'live-paper-template.pdf'), pagesize=(210*mm,297*mm), invariant=1)
    pdf.setTitle('Ecky live paper - A4 100 percent scale')
    pdf.setFont('Helvetica-Bold', 13)
    pdf.drawCentredString(105*mm, 285*mm, 'ECKY / LIVE PAPER')
    pdf.setFont('Helvetica', 8)
    pdf.drawCentredString(105*mm, 280*mm, 'A4 - print at 100% / actual size - keep all four markers visible')
    scale = 4
    sheet = np.full((297*scale,210*scale),255,np.uint8)
    for i,(cx,cy) in enumerate(CENTERS):
        marker = cv2.aruco.generateImageMarker(DICT,i,6)
        for y in range(6):
            for x in range(6):
                if marker[y,x] == 0:
                    pdf.setFillColorRGB(0,0,0)
                    pdf.rect((cx-8+x*16/6)*mm,(297-(cy-8+(y+1)*16/6))*mm,16/6*mm,16/6*mm,stroke=0,fill=1)
        rendered = cv2.resize(marker,(64,64),interpolation=cv2.INTER_NEAREST)
        sheet[int((cy-8)*scale):int((cy+8)*scale),int((cx-8)*scale):int((cx+8)*scale)] = rendered
    pdf.setFillColorRGB(0,0,0)
    pdf.setFont('Helvetica',8)
    pdf.drawCentredString(105*mm, 25*mm, 'Blank drawing region: 160 x 220 mm. Origin: bottom-left.')
    pdf.setStrokeColorRGB(0,0,0)
    pdf.setLineWidth(.6)
    pdf.line(80*mm,15*mm,130*mm,15*mm)
    pdf.line(80*mm,13*mm,80*mm,17*mm)
    pdf.line(130*mm,13*mm,130*mm,17*mm)
    pdf.drawCentredString(105*mm,9*mm,'Scale check: measure this segment = 50 mm')
    pdf.showPage();pdf.save()
    cv2.imwrite(str(FIXTURES/'blank.png'),sheet)
    def pt(x,y): return (round((25+x)*scale),round((258-y)*scale))
    mixed=sheet.copy()
    cv2.line(mixed,pt(15,180),pt(85,180),0,3)
    cv2.rectangle(mixed,pt(15,120),pt(85,160),0,3)
    cv2.circle(mixed,pt(115,140),round(12*scale),0,3)
    cv2.circle(mixed,pt(115,185),3,0,-1)
    cv2.imwrite(str(FIXTURES/'primitives.png'),mixed)
    src=np.float32([[0,0],[840,0],[840,1188],[0,1188]])
    dst=np.float32([[80,45],[850,110],[800,1160],[30,1100]])
    warp=cv2.getPerspectiveTransform(src,dst)
    transformed=cv2.warpPerspective(mixed,warp,(900,1220),borderValue=255)
    rng=np.random.default_rng(2401)
    transformed=np.clip(transformed.astype(np.int16)+rng.normal(0,3,transformed.shape),0,255).astype(np.uint8)
    cv2.imwrite(str(FIXTURES/'perspective-noise.png'),transformed)
    junctions=sheet.copy()
    cv2.line(junctions,pt(20,160),pt(80,160),0,3)
    cv2.line(junctions,pt(50,160),pt(50,200),0,3)
    cv2.line(junctions,pt(95,160),pt(135,200),0,3)
    cv2.line(junctions,pt(95,200),pt(135,160),0,3)
    cv2.line(junctions,pt(20,100),pt(48.5,100),0,3)
    cv2.line(junctions,pt(51,100),pt(80,100),0,3)
    cv2.imwrite(str(FIXTURES/'junctions.png'),junctions)
    missing=mixed.copy();missing[80:160,30:110]=255
    cv2.imwrite(str(FIXTURES/'marker-loss.png'),missing)
    occluded=mixed.copy();cv2.ellipse(occluded,pt(70,140),(160,180),-20,0,360,40,-1)
    cv2.imwrite(str(FIXTURES/'occlusion.png'),occluded)

if __name__ == '__main__': main()
