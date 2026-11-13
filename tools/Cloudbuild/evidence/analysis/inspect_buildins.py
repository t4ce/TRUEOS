#!/usr/bin/env python3
from pathlib import Path
import hashlib, io, json, re, struct, subprocess, sys, tarfile
from analyze_release import ROOT, decode_frame, elf_sections, sha

EXPECTED = ['apps','commander','hull','trauma','ssh','img','os','backup','termdir','edit','share','paper']
USER_TABLE = {
'ssh':(27,63,2920648,1384524), 'hull':(0,101,6882488,3022846), 'share':(0,46,1364960,734913),
'edit':(0,39,1053448,475287), 'trauma':(0,50,682008,378514), 'termdir':(0,53,609032,336525),
'apps':(0,42,234024,125516), 'img':(0,49,217968,128800), 'os':(0,35,174752,92135),
'backup':(0,30,173784,94810), 'commander':(0,31,111240,56633), 'paper':(0,41,40624,24426),
}

def imports(data):
    sections=elf_sections(data)
    results=[]
    for section in sections:
        if section['type'] not in (2,11): continue
        strings=sections[section['link']]
        text=data[strings['offset']:strings['offset']+strings['bytes']]
        for off in range(section['offset'], section['offset']+section['bytes'],section['entry_bytes']):
            st_name,info,other,shndx,value,size=struct.unpack_from('<IBBHQQ',data,off)
            name=text[st_name:].split(b'\0',1)[0].decode('utf8','strict')
            if shndx==0 and name:
                results.append({'name':name,'binding':info>>4,'type':info&15})
    return results

if __name__=='__main__':
    db=(ROOT/'frame-14615049.raw').read_bytes()
    out=[]
    for name in EXPECTED:
        key=(name+'.bp').encode()
        marker=key+b'TRBP'
        pos=0; candidates=[]
        while True:
            pos=db.find(marker,pos)
            if pos<0: break
            package_start=pos+len(key)
            magic,version,flags,entry,payload_size,raw_size=struct.unpack_from('<4sHHQII',db,package_start)
            if version==1 and (flags&0xff)==3 and package_start+24+payload_size<=len(db):
                leaf=pos-12
                tag,flags_leaf,count,keyend,valueend=struct.unpack_from('<BBHII',db,leaf)
                if leaf%4096==0 and (tag,flags_leaf,count)==(1,0,1) and keyend==12+len(key) and valueend==keyend+24+payload_size:
                    candidates.append((package_start,payload_size,raw_size,entry,flags,leaf))
            pos+=len(marker)
        if len(candidates)!=1: raise ValueError(f'{name}: wanted one validated redb leaf value; got {len(candidates)}')
        start,payload_size,raw_size,entry,flags,leaf=candidates[0]
        bp=db[start:start+24+payload_size]
        digest=sha(bp)
        if db.count(digest.encode()) != 1: raise ValueError(f'{name}: exact package hash not uniquely present in DB')
        raw,used=decode_frame(bp[24:],max_output=32*1024*1024)
        if used != payload_size: raise ValueError('extra bytes beyond BP frame')
        with tarfile.open(fileobj=io.BytesIO(raw),mode='r:') as tar:
            members=tar.getmembers()
            if len(members)!=1 or members[0].name!='module.elf' or not members[0].isfile(): raise ValueError('unexpected BP member layout')
            module=tar.extractfile(members[0]).read()
        if len(module)!=raw_size or module[:6] != b'\x7fELF\x02\x01': raise ValueError('ELF length or header mismatch')
        if struct.unpack_from('<HH',module,16) != (1,62): raise ValueError('expected x86-64 relocatable ELF')
        dest=ROOT/'buildins'/name
        dest.mkdir(parents=True,exist_ok=True)
        (dest/'module.elf').write_bytes(module)
        (dest/(name+'.bp')).write_bytes(bp)
        symbols=imports(module)
        symbol_names=sorted(set(x['name'] for x in symbols))
        row={'id':name,'packed_bp_bytes':len(bp),'elf_bytes':len(module),'bp_sha256':digest,'elf_sha256':sha(module),
             'db_leaf_offset':leaf,'db_value_offset':start,'header_flags':flags,'imports':symbols,'all_imports':len(symbol_names),
             'table_packed_bp_bytes':USER_TABLE[name][3],'packed_change_bytes':len(bp)-USER_TABLE[name][3],
             'table_elf_bytes':USER_TABLE[name][2],'elf_change_bytes':len(module)-USER_TABLE[name][2]}
        out.append(row)
        print(json.dumps({k:v for k,v in row.items() if k!='imports'}))
        print('imports',name,','.join(symbol_names))
    (ROOT/'buildin-measurements.json').write_text(json.dumps(out,indent=2)+'\n')
    print(json.dumps({'total_packed_bp_bytes':sum(r['packed_bp_bytes'] for r in out),'table_total_packed_bp_bytes':sum(r['table_packed_bp_bytes'] for r in out),'total_elf_bytes':sum(r['elf_bytes'] for r in out),'kernel_file_minus_embedded_appdb':26213712-5825237}))
