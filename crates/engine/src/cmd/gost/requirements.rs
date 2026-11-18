//! Structured technical requirements, drawn as ordinary numbered TEXT in private blocks.
//! Clean-room behavior references: https://help.ascon.ru/KOMPAS/24/ru-RU/ipr_text_slave.html
//! and https://help.ascon.ru/KOMPAS/24/ru-RU/414_47_2_razbivka_tekhnicheskik.html.
//! Typical wording below is original, not a copy of a vendor's text library.

use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, HAlign, Handle, Insert, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};
use crate::cmd::{CommandSpec, bad};
use super::data;

const CMD: &str = "gost.requirements";
const KIND: &str = "requirements";
const PREFIX: &str = "GOST-REQUIREMENTS-";
const MAX_ITEMS: usize = 64;
const MAX_PAGES: usize = 16;
const MAX_RECORDS: usize = 256;
const MAX_LINES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    name: String,
    text: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
enum Placement {
    Auto,
    Explicit { at: [f64; 2] },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    items: Vec<String>,
    library: BTreeMap<String, Template>,
    placement: Placement,
    width: f64,
    height: f64,
    /// Last generated insertion point: a MOVE/COPY becomes explicit placement.
    anchor: [f64; 2],
}
#[derive(Clone, Debug, PartialEq)]
struct Sheet {
    name: String,
    width: f64,
    height: f64,
    title_top: f64,
}
struct Geometry {
    anchor: Vec2,
    texts: Vec<Text>,
    pages: usize,
    lines: usize,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "GOST Technical Requirements", run)
            .menu(&["Format", "GOST", "GOST Technical Requirements"])
            .params("{handle?: edit existing Insert, layout?: first/selected native GOST sheet, items?: [string|{template:string,values?:{key:string}}], edit?:{index:1-based,item:string|{template,values?}}, automatic?:bool, at?:[x,y] top-left paper mm, width?:mm (175), height?:2.5|3.5|5 (3.5)}; create requires nonempty items; edit preserves omitted fields; columns flow left, overflow is an atomic error")
            .interactive(|_| Ok(Box::new(RequirementsM::default()))),
        CommandSpec::new("gost.requirements.inspect", "Inspect Technical Requirements", inspect)
            .menu(&["Format", "GOST", "Inspect Technical Requirements"])
            .params("{handle?: requirements Insert (otherwise all)} → {requirements:[{handle,layout,record,pages,lines}]}"),
        CommandSpec::new("gost.requirements.templates", "Technical Requirement Templates", templates)
            .menu(&["Format", "GOST", "Technical Requirement Templates"])
            .params("{handle?: requirements Insert, action?:list|set|delete|use (list), id?:string, name?:string, text?:string with {parameter} slots, values?:{key:string}, index?:1-based replacement (use otherwise appends)}; custom templates live in this Insert's DXF metadata; set/delete/use require handle")
            .interactive(|_| Ok(Box::new(TemplatesM))),
    ]
}

fn builtin() -> BTreeMap<String, Template> {
    [
        ("edges", "Кромки", "Неуказанные острые кромки притупить до {size} мм."),
        ("coating", "Покрытие", "Нанести покрытие {coating} на поверхности, отмеченные на чертеже."),
        ("inspection", "Контроль", "Проверить размер {dimension} методом {method}."),
        ("cleaning", "Очистка", "Перед сборкой очистить детали от стружки и технологических загрязнений."),
    ].into_iter().map(|(id, name, text)| (id.into(), Template {name:name.into(), text:text.into()})).collect()
}
fn checked_text(text: &str, limit: usize, what: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > limit || text.chars().any(char::is_control) {
        return Err(bad(CMD, format!("{what} must be nonempty single-line text of at most {limit} UTF-8 bytes")));
    }
    // DXF TEXT interprets %% escapes; reject rather than measure different visible glyphs.
    if text.contains("%%") { return Err(bad(CMD, "DXF %% text escapes are not supported; use literal Unicode symbols")); }
    Ok(())
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn slots(text: &str) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find(['{', '}']) {
        if rest.get(start..start + 1) != Some("{") { return Err(bad(CMD, "unmatched template brace")); }
        let tail = rest.get(start + 1..).ok_or_else(|| bad(CMD, "invalid template"))?;
        let end = tail.find('}').ok_or_else(|| bad(CMD, "unmatched template brace"))?;
        let key = tail.get(..end).ok_or_else(|| bad(CMD, "invalid template slot"))?;
        if !valid_id(key) { return Err(bad(CMD, "template slot names must be ASCII letters, digits, _ or -")); }
        out.insert(key.into());
        if out.len() > 16 { return Err(bad(CMD, "at most 16 template slots")); }
        rest = tail.get(end + 1..).ok_or_else(|| bad(CMD, "invalid template"))?;
    }
    Ok(out)
}
fn validate(r: &Record) -> Result<()> {
    if r.items.is_empty() || r.items.len() > MAX_ITEMS { return Err(bad(CMD, "requirements need 1..64 items")); }
    let mut bytes = 0usize;
    for t in &r.items { checked_text(t, 2048, "item")?; bytes += t.len(); }
    if r.library.len() > 16 { return Err(bad(CMD, "at most 16 custom templates per requirements record")); }
    for (id, t) in &r.library {
        if !valid_id(id) || builtin().contains_key(id) { return Err(bad(CMD, "invalid or reserved custom template id")); }
        checked_text(&t.name, 96, "template name")?;
        checked_text(&t.text, 2048, "template text")?;
        slots(&t.text)?;
        bytes += id.len() + t.name.len() + t.text.len();
    }
    if bytes > 8000 { return Err(bad(CMD, "item and template text budget exceeds 8000 bytes")); }
    if !r.width.is_finite() || !(20.0..=1160.0).contains(&r.width) || ![2.5, 3.5, 5.0].contains(&r.height) {
        return Err(bad(CMD, "width must be 20..1160 mm; text height must be 2.5, 3.5 or 5 mm"));
    }
    let valid_point = |p: &[f64;2]| p.iter().all(|v| v.is_finite() && v.abs() <= 1200.0);
    if !valid_point(&r.anchor) || matches!(&r.placement, Placement::Explicit {at} if !valid_point(at)) {
        return Err(bad(CMD, "placement must be finite paper coordinates within ±1200 mm"));
    }
    // Also preflight the actual serialized DXF payload, including JSON escaping.
    data::save(&mut Common::default(), KIND, r)
}
fn expand(item: &Value, library: &BTreeMap<String, Template>) -> Result<String> {
    if let Some(text) = item.as_str() { checked_text(text, 2048, "item")?; return Ok(text.trim().into()); }
    let object = item.as_object().ok_or_else(|| bad(CMD, "item must be text or {template,values}"))?;
    if object.keys().any(|k| !matches!(k.as_str(), "template" | "values")) { return Err(bad(CMD, "unknown template item parameter")); }
    let id = item.get("template").and_then(Value::as_str).ok_or_else(|| bad(CMD, "template id required"))?;
    let built = builtin();
    let t = library.get(id).or_else(|| built.get(id)).ok_or_else(|| bad(CMD, "template not found"))?;
    let keys = slots(&t.text)?;
    let empty = serde_json::Map::new();
    let values = match item.get("values") { None => &empty, Some(v) => v.as_object().ok_or_else(|| bad(CMD, "values must be string parameters"))? };
    if values.len() != keys.len() || values.keys().any(|k| !keys.contains(k)) { return Err(bad(CMD, "provide exactly the template's named parameters")); }
    for v in values.values() { checked_text(v.as_str().ok_or_else(|| bad(CMD, "template values must be strings"))?, 256, "template value")?; }
    let mut result = String::with_capacity(t.text.len());
    let mut rest = t.text.as_str();
    while let Some(start) = rest.find('{') {
        let tail = rest.get(start + 1..).ok_or_else(|| bad(CMD, "invalid template"))?;
        let end = tail.find('}').ok_or_else(|| bad(CMD, "invalid template"))?;
        let key = tail.get(..end).ok_or_else(|| bad(CMD, "invalid template"))?;
        result.push_str(rest.get(..start).ok_or_else(|| bad(CMD, "invalid template"))?);
        result.push_str(values.get(key).and_then(Value::as_str).ok_or_else(|| bad(CMD, "missing template parameter"))?);
        if result.len() > 2048 { return Err(bad(CMD, "expanded item exceeds 2048 bytes")); }
        rest = tail.get(end + 1..).ok_or_else(|| bad(CMD, "invalid template"))?;
    }
    result.push_str(rest);
    checked_text(&result, 2048, "expanded item")?;
    Ok(result)
}
fn sheet(d: &Drawing, name: Option<&str>) -> Result<Sheet> {
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    for l in layouts {
        if name.is_some_and(|n| !l.name.eq_ignore_ascii_case(n)) { continue; }
        let mut title_top = None;
        for e in l.entities.iter() {
            let EntityKind::Insert(i) = &e.kind else { continue };
            let Some(b) = d.block(&i.block) else { continue };
            let top = if b.description.starts_with("GOST R 2.104-2023 form 1,") { 60.0 }
                else if b.description.starts_with("GOST R 2.104-2023 form 2a,") { 20.0 } else { continue };
            normal_insert(i)?;
            if i.insert != Vec3::ZERO || title_top.replace(top).is_some() { return Err(bad(CMD, "sheet needs exactly one untransformed native GOST frame")); }
        }
        let Some(title_top) = title_top else { continue };
        let (w,h) = if l.page.landscape {(l.page.height_mm,l.page.width_mm)} else {(l.page.width_mm,l.page.height_mm)};
        if !w.is_finite() || !h.is_finite() || !(210.0..=1189.0).contains(&w) || !(210.0..=1189.0).contains(&h)
            || cadcraft_render::paper::page_unit_mm(d, &l.page) != 1.0 {
            return Err(bad(CMD, "requirements require a finite native millimetre GOST sheet"));
        }
        return Ok(Sheet {name:l.name.clone(),width:w,height:h,title_top});
    }
    Err(bad(CMD, "native GOST sheet not found"))
}
fn normal_insert(i: &Insert) -> Result<()> {
    if !i.insert.is_finite() || i.insert.z != 0.0 || i.scale != Vec3::new(1.0,1.0,1.0) || i.rotation != 0.0 || i.rows != 1 || i.cols != 1 || i.clip.is_some() {
        return Err(bad(CMD, "requirements and frame references must be unrotated, unclipped, unit-scale single Inserts in the paper plane"));
    }
    Ok(())
}
fn font(d: &Drawing) -> Result<cadcraft_fonts::TextFont> {
    let st = d.text_style("GOST").ok_or_else(|| bad(CMD, "apply gost.setup first"))?;
    if st.width_factor != 1.0 || st.oblique != 0.0 || st.backwards || st.upside_down || st.vertical || st.annotative {
        return Err(bad(CMD, "GOST text style must have normal unsqueezed horizontal glyphs"));
    }
    let layer = d.layer("GOST-TEXT").ok_or_else(|| bad(CMD, "apply gost.setup first"))?;
    if layer.locked || !layer.visible() || !layer.plot { return Err(bad(CMD, "GOST-TEXT must be unlocked, visible and printable")); }
    Ok(cadcraft_render::style_font(st))
}
fn wrap(text: &str, width: f64, height: f64, font: &cadcraft_fonts::TextFont) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let old = line.len();
        if old > 0 { line.push(' '); }
        line.push_str(word);
        let measured = cadcraft_fonts::text_width(font, &line, height, 1.0);
        if !measured.is_finite() { return Err(bad(CMD, "invalid font metrics")); }
        if measured <= width { continue; }
        line.truncate(old);
        if line.is_empty() { return Err(bad(CMD, format!("word `{word}` exceeds column width; increase width or edit the item"))); }
        lines.push(std::mem::take(&mut line));
        line.push_str(word);
        if cadcraft_fonts::text_width(font, &line, height, 1.0) > width { return Err(bad(CMD, format!("word `{word}` exceeds column width"))); }
    }
    if !line.is_empty() { lines.push(line); }
    Ok(lines)
}
fn native_text(value: String, at: Vec2, height: f64) -> Text {
    Text {insert:at.to3(0.0), align_pt:Some(at.to3(0.0)), height, value, rotation:0.0, width_factor:1.0,
        oblique:0.0, style:"GOST".into(), halign:HAlign::Left, valign:VAlign::Top}
}
fn geometry(d: &Drawing, sh: &Sheet, r: &Record) -> Result<Geometry> {
    validate(r)?;
    let font = font(d)?;
    let indent = cadcraft_fonts::text_width(&font, &format!("{}. ", r.items.len()), r.height, 1.0);
    if !indent.is_finite() || indent >= r.width { return Err(bad(CMD, "numbering does not fit width")); }
    let mut numbered = Vec::new();
    let mut total = 0;
    for (index,item) in r.items.iter().enumerate() {
        let lines = wrap(item, r.width-indent, r.height, &font)?;
        total += lines.len();
        if total > MAX_LINES { return Err(bad(CMD, "requirements exceed 2048 rendered lines")); }
        for (line,text) in lines.into_iter().enumerate() { numbered.push((index+1,line==0,text)); }
    }
    let step = r.height * 1.5;
    let auto = matches!(r.placement, Placement::Auto);
    let initial_x = match r.placement {Placement::Auto => sh.width-10.0-r.width,Placement::Explicit{at}=>at[0]};
    let top = match r.placement {Placement::Auto=>sh.height-10.0,Placement::Explicit{at}=>at[1]};
    if initial_x < 20.0 || initial_x+r.width > sh.width-5.0 || top > sh.height-5.0 {
        return Err(bad(CMD, "requirements placement exceeds GOST frame margins x20..width-5, top height-5"));
    }
    let mut columns = Vec::new();
    let mut x = initial_x;
    let mut remaining = total;
    for _ in 0..MAX_PAGES {
        if remaining == 0 { break; }
        if x < 20.0 { break; }
        let bottom = if x+r.width > sh.width-190.0 { sh.title_top+5.0 } else { 10.0 };
        let capacity = if top >= bottom+r.height { ((top-bottom-r.height)/step).floor() as usize+1 } else { 0 };
        if capacity == 0 { break; }
        let count = remaining.min(capacity);
        // Auto pages sit just above the title block (or bottom frame), never at arbitrary model coordinates.
        let y = if auto { bottom+r.height+(count.saturating_sub(1)) as f64*step } else { top };
        columns.push((Vec2::new(x,y), count));
        remaining -= count;
        x -= r.width+10.0;
    }
    if remaining != 0 { return Err(bad(CMD, format!("technical requirements overflow: {remaining} of {total} wrapped lines do not fit within sheet bounds (maximum {MAX_PAGES} columns); enlarge the sheet, reduce width/height or shorten text"))); }
    let anchor = columns.first().map(|(p,_)|*p).ok_or_else(|| bad(CMD, "no room for requirements"))?;
    let pages = columns.len();
    let mut texts = Vec::with_capacity(total+r.items.len());
    let mut source = numbered.into_iter();
    for (at,count) in columns {
        for row in 0..count {
            let (number,first,text) = source.next().ok_or_else(||bad(CMD,"inconsistent requirements layout"))?;
            let local = at-anchor-Vec2::new(0.0,row as f64*step);
            if first { texts.push(native_text(format!("{number}."),local,r.height)); }
            texts.push(native_text(text,local+Vec2::new(indent,0.0),r.height));
        }
    }
    Ok(Geometry {anchor,texts,pages,lines:total})
}
fn record(s: &Session, h: Handle) -> Result<(Record, Sheet)> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(||bad(CMD,"requirements Insert not found"))?;
    let EntityKind::Insert(i) = &e.kind else {return Err(bad(CMD,"requirements must be an Insert"))};
    normal_insert(i)?;
    let mut r: Record = data::load(&e.common,KIND)?.ok_or_else(||bad(CMD,"not structured technical requirements"))?;
    validate(&r)?;
    if i.insert.xy() != Vec2::new(r.anchor[0],r.anchor[1]) { r.placement=Placement::Explicit{at:[i.insert.x,i.insert.y]}; }
    let name=d.space_of(h).and_then(|sp|sp.layout().map(str::to_string)).ok_or_else(||bad(CMD,"requirements must belong to paper space"))?;
    Ok((r,sheet(d,Some(&name))?))
}
fn handle(p: &Value) -> Result<Option<Handle>> {
    p.get("handle").map(|v|v.as_str().and_then(Handle::parse_hex).or_else(||v.as_u64().map(Handle)).ok_or_else(||bad(CMD,"invalid handle"))).transpose()
}
fn object<'a>(p: &'a Value, allowed: &[&str]) -> Result<&'a serde_json::Map<String,Value>> {
    let o=p.as_object().ok_or_else(||bad(CMD,"parameters must be an object"))?;
    if let Some(k)=o.keys().find(|k|!allowed.contains(&k.as_str())) {return Err(bad(CMD,format!("unknown parameter `{k}`")))}
    Ok(o)
}
fn install(s: &mut Session, h: Option<Handle>, sh: &Sheet, mut r: Record) -> Result<Value> {
    let g=geometry(s.doc()?,sh,&r)?;
    r.anchor=[g.anchor.x,g.anchor.y];
    let mut common=match h {Some(h)=>s.doc()?.entity(h).ok_or_else(||bad(CMD,"requirements missing"))?.common.clone(),None=>Common{layer:"GOST-TEXT".into(),..Common::default()}};
    data::save(&mut common,KIND,&r)?;
    let old_block=h.and_then(|h|s.doc().ok()?.entity(h)).and_then(|e|if let EntityKind::Insert(i)=&e.kind{Some(i.block.clone())}else{None});
    let d=s.doc_mut()?;
    // A fresh block on every write is deliberate copy-on-write: COPY references may share the old definition.
    let name=(0..10_000).map(|n|format!("{PREFIX}{}-{n}",h.map_or_else(||"NEW".into(),Handle::hex))).find(|n|d.block(n).is_none()).ok_or_else(||bad(CMD,"no free requirements block name"))?;
    let mut block=Block::new(&name);
    block.units=4;
    block.description="CADCraftGOST structured technical requirements".into();
    for t in g.texts {block.entities.push(Entity{handle:d.new_handle(),common:Common::default(),kind:EntityKind::Text(t)});}
    d.blocks.insert(name.clone(),Arc::new(block));
    let h=if let Some(h)=h {
        d.modify_entity(h,|e|{e.common=common;if let EntityKind::Insert(i)=&mut e.kind{i.block=name;i.insert=g.anchor.to3(0.0);}})?;
        h
    } else {d.add(&cadcraft_doc::Space::Paper(sh.name.clone()),common,EntityKind::Insert(Insert{block:name,insert:g.anchor.to3(0.0),..Insert::default()}))?};
    if let Some(old)=old_block.filter(|n|n.starts_with(PREFIX)) {
        let used=d.model.iter().chain(d.layouts.iter().flat_map(|l|l.entities.iter())).chain(d.blocks.values().flat_map(|b|b.entities.iter())).any(|e|matches!(&e.kind,EntityKind::Insert(i) if i.block==old));
        if !used {d.blocks.remove(&old);}
    }
    Ok(json!({"handle":h.hex(),"layout":sh.name,"items":r.items.len(),"pages":g.pages,"lines":g.lines,"anchor":r.anchor,"record":r}))
}
fn atomic(s: &mut Session, f: impl FnOnce(&mut Session)->Result<Value>) -> Result<Value> {
    let before=s.state()?.doc.clone();
    let result=f(s);
    if result.is_err(){s.state_mut()?.doc=before;}
    result
}
fn run(s: &mut Session,p: &Value)->Result<Value>{atomic(s,|s|run_inner(s,p))}
fn run_inner(s: &mut Session,p: &Value)->Result<Value>{
    object(p,&["handle","layout","items","edit","automatic","at","width","height"])?;
    let h=handle(p)?;
    let (mut r,mut sh)=if let Some(h)=h {record(s,h)?} else {
        let name=p.get("layout").map(|v|v.as_str().ok_or_else(||bad(CMD,"layout must be a string"))).transpose()?;
        let selected=s.space();
        let selected=selected.layout().filter(|n|sheet(s.doc().ok()?,Some(n)).is_ok());
        (Record{items:Vec::new(),library:BTreeMap::new(),placement:Placement::Auto,width:175.0,height:3.5,anchor:[0.0,0.0]},sheet(s.doc()?,name.or(selected))?)
    };
    if let Some(v)=p.get("layout") {
        let requested=sheet(s.doc()?,Some(v.as_str().ok_or_else(||bad(CMD,"layout must be a string"))?))?;
        if h.is_some() && requested.name!=sh.name {return Err(bad(CMD,"editing cannot move requirements between layouts; COPY to the target sheet first"))}
        sh=requested;
    }
    if let Some(items)=p.get("items") {
        let items=items.as_array().filter(|a|!a.is_empty()&&a.len()<=MAX_ITEMS).ok_or_else(||bad(CMD,"items must contain 1..64 entries"))?;
        r.items=items.iter().map(|i|expand(i,&r.library)).collect::<Result<_>>()?;
    }
    if let Some(edit)=p.get("edit") {
        object(edit,&["index","item"])?;
        let index=edit.get("index").and_then(Value::as_u64).filter(|i|*i>=1&&*i<=r.items.len() as u64).ok_or_else(||bad(CMD,"edit index is 1-based and must exist"))? as usize-1;
        let text=expand(edit.get("item").ok_or_else(||bad(CMD,"edit item required"))?,&r.library)?;
        let item=r.items.get_mut(index).ok_or_else(||bad(CMD,"item not found"))?;
        *item=text;
    }
    if let Some(v)=p.get("width"){r.width=v.as_f64().ok_or_else(||bad(CMD,"width must be a number"))?;}
    if let Some(v)=p.get("height"){r.height=v.as_f64().ok_or_else(||bad(CMD,"height must be a number"))?;}
    if let Some(v)=p.get("automatic") {
        if v.as_bool().ok_or_else(||bad(CMD,"automatic must be boolean"))? {r.placement=Placement::Auto;}
        else if p.get("at").is_none() {r.placement=Placement::Explicit{at:r.anchor};}
    }
    if let Some(v)=p.get("at") {
        if p.get("automatic")==Some(&Value::Bool(true)){return Err(bad(CMD,"at and automatic:true are mutually exclusive"))}
        let a=v.as_array().filter(|a|a.len()==2).ok_or_else(||bad(CMD,"at must be [x,y] top-left"))?;
        let x=a.first().and_then(Value::as_f64).ok_or_else(||bad(CMD,"invalid x"))?;
        let y=a.get(1).and_then(Value::as_f64).ok_or_else(||bad(CMD,"invalid y"))?;
        r.placement=Placement::Explicit{at:[x,y]};
    }
    install(s,h,&sh,r)
}
fn inspect(s:&mut Session,p:&Value)->Result<Value>{
    object(p,&["handle"])?;
    let requested=handle(p)?;
    let mut result=Vec::new();
    for l in &s.doc()?.layouts {for e in l.entities.iter(){
        if requested.is_some_and(|h|h!=e.handle){continue;}
        if data::load::<Record>(&e.common,KIND)?.is_none(){continue;}
        if result.len()>=MAX_RECORDS{return Err(bad(CMD,"at most 256 requirements records"))}
        let (r,sh)=record(s,e.handle)?;
        let g=geometry(s.doc()?,&sh,&r)?;
        result.push(json!({"handle":e.handle.hex(),"layout":l.name,"record":r,"pages":g.pages,"lines":g.lines}));
    }}
    if requested.is_some()&&result.is_empty(){return Err(bad(CMD,"requirements not found"))}
    Ok(json!({"requirements":result}))
}
fn templates(s:&mut Session,p:&Value)->Result<Value>{atomic(s,|s|templates_inner(s,p))}
fn templates_inner(s:&mut Session,p:&Value)->Result<Value>{
    object(p,&["handle","action","id","name","text","values","index"])?;
    let action=p.get("action").map_or(Ok("list"),|v|v.as_str().ok_or_else(||bad(CMD,"action must be a string")))?;
    let h=handle(p)?;
    let existing=h.map(|h|record(s,h)).transpose()?;
    if action=="list" {
        let mut library=builtin();
        if let Some((r,_))=&existing{library.extend(r.library.clone());}
        let entries:Vec<_>=library.iter().map(|(id,t)|Ok(json!({"id":id,"name":t.name,"text":t.text,"parameters":slots(&t.text)?,"builtin":builtin().contains_key(id)}))).collect::<Result<_>>()?;
        return Ok(json!({"templates":entries}));
    }
    let h=h.ok_or_else(||bad(CMD,"template changes and use require a requirements handle"))?;
    let (mut r,sh)=existing.ok_or_else(||bad(CMD,"requirements missing"))?;
    let id=p.get("id").and_then(Value::as_str).filter(|id|valid_id(id)).ok_or_else(||bad(CMD,"valid template id required"))?;
    match action {
        "set"=>{
            if builtin().contains_key(id){return Err(bad(CMD,"built-in templates are read-only; choose a custom id"))}
            let name=p.get("name").and_then(Value::as_str).ok_or_else(||bad(CMD,"template name required"))?;
            let text=p.get("text").and_then(Value::as_str).ok_or_else(||bad(CMD,"template text required"))?;
            r.library.insert(id.into(),Template{name:name.into(),text:text.into()});
        },
        "delete"=>{if r.library.remove(id).is_none(){return Err(bad(CMD,"custom template not found"))}},
        "use"=>{
            let mut item=json!({"template":id});
            if let Some(v)=p.get("values"){item["values"]=v.clone();}
            let text=expand(&item,&r.library)?;
            if let Some(index)=p.get("index") {
                let index=index.as_u64().filter(|i|*i>=1&&*i<=r.items.len() as u64).ok_or_else(||bad(CMD,"index must identify an existing item (1-based)"))? as usize-1;
                *r.items.get_mut(index).ok_or_else(||bad(CMD,"item not found"))?=text;
            }else{r.items.push(text);}
        },
        _=>return Err(bad(CMD,"action must be list, set, delete or use")),
    }
    install(s,Some(h),&sh,r)
}

/// Reflow against real page/form/font changes in the surrounding command's undo transaction.
/// A copied/moved Insert owns its independent metadata and becomes explicitly positioned.
pub(super) fn after_command(s:&mut Session,before:Option<&Arc<Drawing>>,uid:Option<u64>)->Result<()> {
    let Some(before)=before else{return Ok(())};
    if uid.is_some_and(|u|s.state().is_ok_and(|st|u!=st.uid))||Arc::ptr_eq(before,&s.state()?.doc){return Ok(())}
    let mut changes=Vec::new();
    let mut count=0;
    let d=s.doc()?;
    for l in &d.layouts {for e in l.entities.iter(){
        let Some(stored)=data::load::<Record>(&e.common,KIND)? else{continue};
        count+=1;
        if count>MAX_RECORDS{return Err(bad(CMD,"at most 256 requirements records"))}
        let (r,sh)=record(s,e.handle)?;
        let prior=before.entity(e.handle);
        let sheet_changed=sheet(before,Some(&l.name)).is_err_or(|old|old!=sh);
        let font_changed=before.text_style("GOST")!=d.text_style("GOST");
        if sheet_changed||font_changed||r!=stored||prior.is_none(){
            // Prepare every layout before mutating any definition: true overflow is atomic.
            geometry(d,&sh,&r)?;
            changes.push((e.handle,sh,r));
        }
    }}
    for (h,sh,r) in changes {install(s,Some(h),&sh,r)?;}
    Ok(())
}

#[derive(Default)]
struct RequirementsM {items:Vec<String>}
impl Interactive for RequirementsM {
    fn name(&self)->&'static str{"GOST.REQUIREMENTS"}
    fn prompt(&self,_:&Session)->Prompt{Prompt::new("Enter next technical requirement; Enter finishes (automatic sheet placement)",Accept::TEXT)}
    fn input(&mut self,s:&mut Session,input:Input)->Result<Step>{
        match input {
            Input::Text(t)=>{checked_text(&t,2048,"item")?;if self.items.len()>=MAX_ITEMS{return Err(bad(CMD,"at most 64 items"))}self.items.push(t);},
            Input::Enter=>{run(s,&json!({"items":self.items}))?;return Ok(Step::Done);},
            _=>{},
        }
        Ok(Step::Continue)
    }
}
struct TemplatesM;
impl Interactive for TemplatesM {
    fn name(&self)->&'static str{"GOST.REQUIREMENTS.TEMPLATES"}
    fn prompt(&self,_:&Session)->Prompt{Prompt::new("Enter template operation JSON: {handle,action:list|set|delete|use,id,...}",Accept::TEXT)}
    fn input(&mut self,s:&mut Session,input:Input)->Result<Step>{
        if let Input::Text(text)=input{
            if text.len()>12000{return Err(bad(CMD,"template command exceeds 12000 bytes"))}
            let p:Value=serde_json::from_str(&text).map_err(|e|bad(CMD,e.to_string()))?;
            let result=templates(s,&p)?;
            s.log(result.to_string());
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
#[path="requirements_tests.rs"]
mod tests;
