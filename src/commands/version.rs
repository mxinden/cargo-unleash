use std::{
    collections::HashMap,
    error::Error
};
use cargo::core::{
    package::Package, Workspace
};
use toml_edit::{Item, Value, decorated};
use semver::{Version, VersionReq};
use crate::util::{edit_each, edit_each_dep, DependencyEntry};
use log::trace;

fn check_for_update<'a>(
    name: String,
    wrap: DependencyEntry<'a>,
    updates: &HashMap<String, Version>
) -> bool {
    let new_version = if let Some(v) = updates.get(&name) {
        v
    } else {
        return false // we do not care about this entry
    };

    match wrap {
        DependencyEntry::Inline(info) => {

            if !info.contains_key("path") {
                return false // entry isn't local
            }

            trace!("We changed the version of {:} to {:}", name, new_version);
            // this has been changed.
            if let Some(v_req) = info.get_mut("version") {
                let r = v_req
                    .as_str()
                    .ok_or("Version must be string".to_owned())
                    .and_then(|s| VersionReq::parse(s)
                        .map_err(|e| format!("Parsing failed {:}", e)))
                    .expect("Cargo enforces us using semver versions. qed");
                if !r.matches(new_version) {
                    trace!("Versions don't match anymore, updating.");
                    *v_req = decorated(Value::from(format!("{:}", new_version)), " ", "");
                    return true
                }
            } else {
                // not yet present, we force set.
                trace!("No version found, setting.");
                // having a space here means we formatting it nicer inline
                info.get_or_insert(" version", decorated(
                        Value::from(format!("{:}", new_version)), " ", " "));
                return true
            }
        },
        DependencyEntry::Table(info) => {
            if !info.contains_key("path") {
                return false // entry isn't local
            }
            if let Some(new_version) = updates.get(&name) {
                trace!("We changed the version of {:} to {:}", name, new_version);
                // this has been changed.
                if let Some(v_req) = info.get("version") {
                    let r = v_req
                        .as_str()
                        .ok_or("Version must be string".to_owned())
                        .and_then(|s| VersionReq::parse(s)
                            .map_err(|e| format!("Parsing failed {:}", e)))
                        .expect("Cargo enforces us using semver versions. qed");
                    if r.matches(new_version) {
                        return false
                    }
                    trace!("Versions don't match anymore, updating.");
                } else {
                    trace!("No version found, setting.");
                }
                info["version"] = Item::Value(decorated(
                        Value::from(format!("{:}", new_version)), " ", ""));
                return true
            }
        }
    }
    false
}

/// For packages matching predicate set to mapper given version, if any. Update all members dependencies
/// if necessary.
pub fn set_version<'a, M, P>(ws: &Workspace<'a>, predicate: P, mapper: M) -> Result<(), Box<dyn Error>>
where
    P: Fn(&Package) -> bool,
    M: Fn(&Package) -> Option<Version>,
{
    let c = ws.config();

    let updates = edit_each(
        ws.members().filter(|p| predicate(p)),
        |p, doc| Ok(mapper(p).map(|nv_version| {
            c.shell()
                .status("Bumping", format!("{:}: {:} -> {:}", p.name(), p.version(), nv_version))
                .expect("Writing to the shell would have failed before. qed");
            doc["package"]["version"] = Item::Value(decorated(
                    Value::from(nv_version.to_string()), " ", ""));
            (p.name().as_str().to_owned(), nv_version.clone())
        }))
    )?
        .into_iter()
        .filter_map(|s| s)
        .collect::<HashMap<_,_>>();

    c.shell().status("Updating", "Dependency tree")?;
    edit_each(ws.members(), |p, doc| {
        c.shell().status("Updating", p.name())?;
        let root = doc.as_table_mut();
        let mut updates_count = 0;
        updates_count += edit_each_dep(root, |a, b| check_for_update(a, b, &updates));
        
        if let Item::Table(t) = root.entry("target") {
            let keys = t.iter().filter_map(|(k, v)| {
                if v.is_table() {
                    Some(k.to_owned())
                } else {
                    None
                }
            }).collect::<Vec<_>>();
            
            for k in keys {
                if let Item::Table(root) = t.entry(&k) {
                    updates_count += edit_each_dep(root,  |a, b| check_for_update(a, b, &updates));
                }
            };
        }
        if updates_count == 0 {
            c.shell().status("Done", "No dependency updates")?;
            
        } else if updates_count == 1 {
            c.shell().status("Done", "One dependency updated")?;
        } else {
            c.shell().status("Done", format!("{} dependencies updated", updates_count))?;
        }



        Ok(())
    })?;

    Ok(())
}