"""Install source-LOD authoring hooks into the isolated renderer only."""
from pathlib import Path
import shutil

HERE=Path(__file__).resolve().parent
SRC=HERE.parents[1]/'captures/rift-texture-review/viewer-build/src'
def apply():
    from source_skin_reference import extract
    extract()
    shutil.copy2(HERE/'review_inventory.inc.rs',SRC/'review_inventory.inc.rs')
    shutil.copy2(HERE/'review_geometry.inc.rs',SRC/'review_geometry.inc.rs')
    shutil.copy2(HERE/'review_source.inc.rs',SRC/'review_source.inc.rs')
    path=SRC/'mobies.rs'
    text=path.read_text()
    if 'include!("review_lod.inc.rs")' not in text:
        anchor='struct Run {'
        if text.count(anchor)!=1:raise ValueError('Run hook changed')
        text=text.replace(anchor,'#[derive(Clone)]\n'+anchor)
        anchor='        let mesh_time = walk_timed().then(std::time::Instant::now);'
        if text.count(anchor)!=1:raise ValueError('Skin hook changed')
        text=text.replace(anchor,'        if crate::material_review::near_source() { runs=self.review_near_runs(runs,core,skin)?; }\n'+anchor)
        text+='\ninclude!("review_lod.inc.rs");\n'
        path.write_text(text)
    shutil.copy2(HERE/'review_lod.inc.rs',SRC/'review_lod.inc.rs')
    path=SRC/'native.rs';text=path.read_text()
    old='&self.boot, mo, ram, jobs, &g.mobies, &cam, &fog, &self.objects,'
    new='&self.boot, mo, ram, jobs, &g.mobies, &g.shrubs, &cam, &fog, &self.objects,'
    if old in text:text=text.replace(old,new)
    elif new not in text:raise ValueError('Review lighting hook changed')
    if 'include!("review_terrain.inc.rs")' not in text:
        text+='\ninclude!("review_terrain.inc.rs");\n'
    path.write_text(text)
    shutil.copy2(HERE/'review_terrain.inc.rs',SRC/'review_terrain.inc.rs')
if __name__=='__main__':apply()
