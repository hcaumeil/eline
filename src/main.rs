use std::collections::{HashMap, HashSet};
use std::process::exit;

use graphviz_rust::cmd::Format;
use graphviz_rust::dot_structures::{Edge, EdgeTy, Graph, Id, Node, NodeId, Stmt, Vertex};
use graphviz_rust::printer::PrinterContext;
use graphviz_rust::{exec_dot, print};
use paludis_rs::{DependenciesLabel, DependencySpecTree, Environment, PackageID};

fn authorized_labels(labels: &Vec<DependenciesLabel>) -> bool {
    let labels = labels
        .iter()
        .map(|l| l.text().to_string())
        .collect::<Vec<_>>();
    return !(labels.contains(&String::from("test"))
        || labels.contains(&String::from("suggestion"))
        || labels.contains(&String::from("test-expensive"))
        || labels.contains(&String::from("built-against")));
}

fn clean_deps(deps: Vec<DependencySpecTree>) -> Vec<DependencySpecTree> {
    let mut res = Vec::new();
    let mut skip = false;

    deps.into_iter().for_each(|d: DependencySpecTree| {
        if skip {
            if let DependencySpecTree::Labels(labels) = d {
                if authorized_labels(&labels) {
                    skip = false;
                    res.push(DependencySpecTree::Labels(labels));
                }
            }
        } else {
            if let DependencySpecTree::Labels(labels) = d {
                if !authorized_labels(&labels) {
                    skip = true;
                } else {
                    res.push(DependencySpecTree::Labels(labels));
                }
            } else {
                res.push(d);
            }
        }
    });

    res
}

fn _dep_fold<N, E>(
    pkg_name: &str,
    pkg_dep: DependencySpecTree,
    packages: &HashMap<String, PackageID>,
    node_fn: fn(String) -> N,
    edge_fn: fn(String, String) -> E,
    nodes: &mut Vec<N>,
    edges: &mut Vec<E>,
    depth: usize,
    depth_max: usize,
    mark: &mut HashSet<String>,
) {
    match pkg_dep {
        paludis_rs::DependencySpecTree::None => {}
        paludis_rs::DependencySpecTree::NamedSet(_) => {}
        paludis_rs::DependencySpecTree::Labels(_) => {}
        paludis_rs::DependencySpecTree::Package(p) => {
            let name = p.full_name();
            if !name.starts_with("user/") && !name.starts_with("group/") {
                edges.push(edge_fn(pkg_name.to_string(), name));

                let pid = packages.get(&p.full_name());
                if let Some(pid) = pid {
                    dep_fold(
                        pid, packages, node_fn, edge_fn, nodes, edges, depth, depth_max, mark,
                    );
                }
            }
        }
        DependencySpecTree::Conditional(_, _) => {}
        paludis_rs::DependencySpecTree::All(all) => {
            let all = clean_deps(all);
            for a in all {
                _dep_fold(
                    pkg_name, a, packages, node_fn, edge_fn, nodes, edges, depth, depth_max, mark,
                );
            }
        }
    }
}

fn dep_fold<N, E>(
    package: &PackageID,
    packages: &HashMap<String, PackageID>,
    node_fn: fn(String) -> N,
    edge_fn: fn(String, String) -> E,
    nodes: &mut Vec<N>,
    edges: &mut Vec<E>,
    depth: usize,
    depth_max: usize,
    mark: &mut HashSet<String>,
) {
    let name = package.name();
    if mark.contains(&name) {
        return;
    } else {
        mark.insert(name.clone());
    }

    nodes.push(node_fn(package.name()));

    if depth == depth_max {
        return;
    }

    if let Some(key) = package.metadata_key("DEPENDENCIES") {
        match key.value() {
            paludis_rs::MetadataValue::DependencySpecTree(d) => _dep_fold(
                &name,
                d,
                packages,
                node_fn,
                edge_fn,
                nodes,
                edges,
                depth + 1,
                depth_max,
                mark,
            ),
            _ => {}
        }
    }
}

fn graphiz_escape(content: &str) -> String {
    "\"".to_owned() + content + "\""
}

fn node_maker(node: String) -> Stmt {
    println!("package : {}", node);
    let node = graphiz_escape(&node);

    Stmt::Node(Node {
        id: NodeId(Id::Plain(node), None),
        attributes: Vec::new(),
    })
}

fn edge_maker(from: String, to: String) -> Stmt {
    let from = graphiz_escape(&from);
    let to = graphiz_escape(&to);

    Stmt::Edge(Edge {
        ty: EdgeTy::Pair(
            Vertex::N(NodeId(Id::Plain(from), None)),
            Vertex::N(NodeId(Id::Plain(to), None)),
        ),
        attributes: Vec::new(),
    })
}

fn best_id_for(mut ids: Vec<PackageID>) -> Option<PackageID> {
    ids.sort_by_key(|id| id.version());

    if ids.len() == 0 {
        return None;
    }

    let allscm = ids
        .iter()
        .fold(true, |acc, id| id.version().is_scm() && acc);

    if allscm {
        ids.pop()
    } else {
        ids.retain(|id| !id.version().is_scm());
        ids.pop()
    }
}

fn package_graph(
    package: &str,
    packages: &mut HashMap<String, PackageID>,
    depth: usize,
) -> (Option<Graph>, usize) {
    let mut nodes: Vec<Stmt> = Vec::new();
    let mut edges: Vec<Stmt> = Vec::new();
    let mut mark: HashSet<String> = HashSet::new();

    let pid = packages.get(package);
    if let Some(pid) = pid {
        println!("{:?}", pid.metadata_key("DEPENDENCIES").map(|v| v.value()));
        println!("\n\n");
        println!(
            "{:?}",
            pid.metadata_key("DEPENDENCIES").map(|v| v.value_str())
        );

        dep_fold(
            &pid, packages, node_maker, edge_maker, &mut nodes, &mut edges, 0, depth, &mut mark,
        );
    } else {
        return (None, 0);
    }

    let dep_number = (nodes.len() - 1).max(0);
    nodes.append(&mut edges);
    let graph = Graph::Graph {
        id: Id::Plain(graphiz_escape(package)),
        strict: false,
        stmts: nodes,
    };

    (Some(graph), dep_number)
}

fn main() {
    let package = "dev-texlive/texlive-xetex";
    let mut packages = HashMap::new();
    let e = Environment::default();

    for r in e.repositories_names() {
        if r != "installed"
            && r != "accounts"
            && r != "graveyard"
            && r != "unavailable"
            && r != "unavailable-unofficial"
            && r != "unwritten"
            && r != "graveyard"
            && r != "repository"
            && r != "installed-accounts"
            && r != "installed_unpackaged"
        {
            let repo = e.fetch_repository(&r).unwrap();
            for p in repo.package_names() {
                if !packages.contains_key(&p) {
                    if let Some(pck) = best_id_for(repo.package_ids(&p)) {
                        packages.insert(p, pck);
                    }
                }
            }
        }
    }

    let (graph, dep_num) = package_graph(package, &mut packages, 132);
    println!("\n{} dependencies found", dep_num);

    if let Some(graph) = graph {
        let s = print(graph, &mut PrinterContext::default());

        let output = package.replace("/", "-");
        _ = std::fs::write(output.clone() + ".dot", s.as_str());

        let graph_svg = exec_dot(s, vec![Format::Svg.into()]).unwrap();
        _ = std::fs::write(output + ".svg", graph_svg);
    } else {
        eprintln!("error: {} not found !", package);
        exit(1);
    }
}
