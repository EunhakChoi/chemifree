import assert from 'node:assert/strict'
import {deflateSync,inflateSync} from 'node:zlib'

function crc(bytes){let value=0xffffffff;for(const byte of bytes){value^=byte;for(let bit=0;bit<8;bit++)value=(value>>>1)^((value&1)?0xedb88320:0)}return(value^0xffffffff)>>>0}
export function pngChunk(type,data=Buffer.alloc(0)){const body=Buffer.concat([Buffer.from(type),data]);const chunk=Buffer.alloc(data.length+12);chunk.writeUInt32BE(data.length,0);body.copy(chunk,4);chunk.writeUInt32BE(crc(body),chunk.length-4);return chunk}

/** Only headerWidth/headerHeight forge large dimensions; raw pixel allocation stays explicit and bounded. */
export function pngFixture({width=2,height=2,headerWidth=width,headerHeight=height,color=6,depth=8,interlace=0,extra=[]}={}){
 assert.ok(width*height<=1048576,'fixtures never allocate beyond one supported image')
 const header=Buffer.alloc(13);header.writeUInt32BE(headerWidth,0);header.writeUInt32BE(headerHeight,4);header[8]=depth;header[9]=color;header[12]=interlace
 const channels=color===2?3:4;const raw=Buffer.alloc(height*(1+width*channels))
 for(let y=0;y<height;y++)for(let x=0;x<width;x++){const offset=y*(1+width*channels)+1+x*channels;if(x<width/2){raw[offset]=255;if(channels===4)raw[offset+3]=255}}
 return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),pngChunk('IHDR',header),...extra,pngChunk('IDAT',deflateSync(raw)),pngChunk('IEND')])
}

/** Read real Chromium screenshots (8-bit RGB/RGBA) so assertions inspect on-screen image pixels. */
export function screenshotPixels(png){
 let width,height,channels;const chunks=[]
 for(let at=8;at+12<=png.length;){const length=png.readUInt32BE(at);const type=png.toString('ascii',at+4,at+8);const data=png.subarray(at+8,at+8+length);if(type==='IHDR'){width=data.readUInt32BE(0);height=data.readUInt32BE(4);assert.equal(data[8],8);channels=data[9]===6?4:data[9]===2?3:0;assert.ok(channels);assert.equal(data[12],0)}if(type==='IDAT')chunks.push(data);at+=length+12}
 assert.ok(width*height<=1048576,'screenshot fixtures stay small')
 const raw=inflateSync(Buffer.concat(chunks));const stride=width*channels;const pixels=Buffer.alloc(height*stride)
 const paeth=(a,b,c)=>{const p=a+b-c;const da=Math.abs(p-a),db=Math.abs(p-b),dc=Math.abs(p-c);return da<=db&&da<=dc?a:db<=dc?b:c}
 for(let y=0;y<height;y++){const filter=raw[y*(stride+1)];for(let x=0;x<stride;x++){const a=x>=channels?pixels[y*stride+x-channels]:0;const b=y?pixels[(y-1)*stride+x]:0;const c=y&&x>=channels?pixels[(y-1)*stride+x-channels]:0;const predict=[0,a,b,Math.floor((a+b)/2),paeth(a,b,c)][filter];assert.notEqual(predict,undefined);pixels[y*stride+x]=(raw[y*(stride+1)+1+x]+predict)&255}}
 return{width,height,at(x,y){const start=(y*width+x)*channels;return[...pixels.subarray(start,start+3)]}}
}
