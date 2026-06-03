use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::security::{check_file_size, MAX_GRAPH_FILE_BYTES};

pub const DEFAULT_MAX_CHILDREN: usize = 200;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub fn common_root(paths: &[&str]) -> String {
    let parts: Vec<Vec<String>> = paths
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            Path::new(p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .collect();

    if parts.is_empty() {
        return String::new();
    }

    let mut common = parts[0].clone();
    for part in &parts[1..] {
        let i = common
            .iter()
            .zip(part.iter())
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(i);
    }

    if common.is_empty() {
        return String::new();
    }

    let mut pb = PathBuf::new();
    for c in &common {
        pb.push(c);
    }
    pb.to_string_lossy().into_owned()
}

pub fn make_truncation_leaf(extra: usize) -> Value {
    json!({
        "name": format!("(+{} more)", extra),
        "total_count": extra,
        "children": []
    })
}

fn finalise(node: &mut Value) -> u64 {
    let kids_len = node
        .get("children")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if kids_len == 0 {
        return node
            .get("total_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
    }

    let mut total = 0u64;
    if let Some(arr) = node.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in arr.iter_mut() {
            total += finalise(child);
        }
        arr.sort_by(|a, b| {
            let a_has_kids = a
                .get("children")
                .and_then(|c| c.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            let b_has_kids = b
                .get("children")
                .and_then(|c| c.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            let a_name = a
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let b_name = b
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let a_ord = if a_has_kids { 0usize } else { 1 };
            let b_ord = if b_has_kids { 0usize } else { 1 };
            a_ord.cmp(&b_ord).then(a_name.cmp(&b_name))
        });
    }
    let total = total.max(1);
    if let Some(v) = node.get_mut("total_count") {
        *v = json!(total);
    }
    total
}

pub fn build_tree(
    graph: &Value,
    root: Option<&str>,
    max_children: usize,
    project_label: Option<&str>,
) -> Value {
    let nodes = match graph.get("nodes").and_then(|n| n.as_array()) {
        Some(n) => n,
        None => {
            return json!({"name": "(empty graph)", "total_count": 0, "children": []});
        }
    };

    let file_nodes: Vec<&Value> = nodes
        .iter()
        .filter(|n| {
            n.get("source_file")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        })
        .collect();

    if file_nodes.is_empty() {
        return json!({"name": "(empty graph)", "total_count": 0, "children": []});
    }

    let source_files: Vec<&str> = file_nodes
        .iter()
        .filter_map(|n| n.get("source_file").and_then(|v| v.as_str()))
        .collect();

    let resolved_root = match root {
        Some(r) => r.to_string(),
        None => common_root(&source_files),
    };
    let root_path = PathBuf::from(&resolved_root);

    let label_root = project_label
        .map(|s| s.to_string())
        .or_else(|| {
            root_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| resolved_root.clone());
    let label_root = if label_root.is_empty() {
        "/".to_string()
    } else {
        label_root
    };

    let mut by_file: HashMap<&str, Vec<&Value>> = HashMap::new();
    for n in &file_nodes {
        let src = n.get("source_file").and_then(|v| v.as_str()).unwrap_or("");
        by_file.entry(src).or_default().push(n);
    }

    // dir_index: path string → mutable node stored in a flat vec; we'll build a tree at the end
    // Use an index-based approach: Vec of (path_string, parent_idx, name, children_indices)
    struct DirEntry {
        name: String,
        children_dirs: Vec<usize>,
        children_files: Vec<Value>,
    }

    let mut dir_entries: Vec<DirEntry> = Vec::new();
    let mut path_to_idx: HashMap<String, usize> = HashMap::new();

    let root_key = root_path.to_string_lossy().into_owned();
    dir_entries.push(DirEntry {
        name: label_root,
        children_dirs: Vec::new(),
        children_files: Vec::new(),
    });
    path_to_idx.insert(root_key.clone(), 0);

    fn ensure_dir(
        abs_path: &Path,
        root_path: &Path,
        dir_entries: &mut Vec<DirEntry>,
        path_to_idx: &mut HashMap<String, usize>,
    ) -> usize {
        let key = abs_path.to_string_lossy().into_owned();
        if let Some(&idx) = path_to_idx.get(&key) {
            return idx;
        }
        if abs_path == abs_path.parent().unwrap_or(abs_path) || abs_path == root_path {
            return 0;
        }
        let parent_path = abs_path.parent().unwrap_or(root_path);
        let parent_idx = ensure_dir(parent_path, root_path, dir_entries, path_to_idx);
        let name = abs_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let idx = dir_entries.len();
        dir_entries.push(DirEntry {
            name,
            children_dirs: Vec::new(),
            children_files: Vec::new(),
        });
        path_to_idx.insert(key, idx);
        dir_entries[parent_idx].children_dirs.push(idx);
        idx
    }

    let mut sorted_files: Vec<(&str, Vec<&Value>)> = by_file.into_iter().collect();
    sorted_files.sort_by_key(|(k, _)| *k);

    for (src_file, syms) in &sorted_files {
        let src_path = Path::new(src_file);
        let parent_path = match src_path.strip_prefix(&root_path) {
            Ok(rel) => root_path
                .join(rel)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| root_path.clone()),
            Err(_) => root_path.clone(),
        };

        let dir_idx = ensure_dir(&parent_path, &root_path, &mut dir_entries, &mut path_to_idx);

        let file_name = src_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| src_file.to_string());

        let mut sym_children: Vec<Value> = syms
            .iter()
            .filter_map(|n| {
                let label = n
                    .get("label")
                    .or_else(|| n.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let file_type = n.get("file_type").and_then(|v| v.as_str()).unwrap_or("");
                if label == file_name && file_type == "code" {
                    return None;
                }
                Some(json!({"name": label, "total_count": 1, "children": []}))
            })
            .collect();

        sym_children.sort_by(|a, b| {
            let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let a_priv = a_name.starts_with('_');
            let b_priv = b_name.starts_with('_');
            a_priv
                .cmp(&b_priv)
                .then(a_name.to_lowercase().cmp(&b_name.to_lowercase()))
        });

        if sym_children.len() > max_children {
            let extra = sym_children.len() - max_children;
            sym_children.truncate(max_children);
            sym_children.push(make_truncation_leaf(extra));
        }

        let total = if sym_children.is_empty() {
            1
        } else {
            sym_children.len() as u64
        };
        let file_node = json!({
            "name": file_name,
            "total_count": total,
            "children": sym_children
        });
        dir_entries[dir_idx].children_files.push(file_node);
    }

    fn to_value(idx: usize, dir_entries: &[DirEntry]) -> Value {
        let entry = &dir_entries[idx];
        let mut children: Vec<Value> = entry
            .children_dirs
            .iter()
            .map(|&cidx| to_value(cidx, dir_entries))
            .collect();
        children.extend(entry.children_files.iter().cloned());

        let mut node = json!({
            "name": entry.name,
            "total_count": 0,
            "children": children
        });
        finalise(&mut node);
        node
    }

    to_value(0, &dir_entries)
}

static HTML_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>__TITLE__</title>
  <style>
    body {
      font-family: 'Segoe UI', sans-serif;
      margin: 0;
      padding: 0;
      background: #f9f9f9;
      color: #333;
    }
    h1 {
      margin: 20px 0 0 24px;
      font-size: 2.2rem;
      font-weight: bold;
      color: #1e3a56;
    }
    .controls {
      margin: 20px 0 15px 24px;
    }
    button {
      margin-right: 10px;
      padding: 8px 18px;
      background: #007bff;
      color: #fff;
      border: none;
      border-radius: 5px;
      font-size: 0.95rem;
      cursor: pointer;
      transition: background 0.2s ease-in-out;
      box-shadow: 0 2px 4px rgba(0,0,0,0.1);
    }
    button:hover { background: #0056b3; }
    button:active { background: #004085; }
    #tree-container {
      width: calc(100vw - 48px);
      height: 85vh;
      overflow: auto;
      border-radius: 8px;
      background: #fff;
      margin-left: 24px;
      margin-right: 24px;
      box-shadow: 0 4px 12px rgba(0,0,0,0.08);
      border: 1px solid #ddd;
    }
    svg {
      background: #fff;
      border-radius: 8px;
      display: block;
    }
    .node circle { stroke-width: 2.5px; }
    .node text {
      font: 13px 'Segoe UI', sans-serif;
      paint-order: stroke fill;
      stroke: #fff;
      stroke-width: 3px;
      stroke-linejoin: round;
      stroke-opacity: 0.85;
    }
    .link {
      fill: none;
      stroke-opacity: 0.7;
      stroke-width: 2px;
    }
  </style>
</head>
<body>
  <h1>__HEADER__</h1>
  <div class="controls">
    <button onclick="expandAll()">Expand All</button>
    <button onclick="collapseAll()">Collapse All</button>
    <button onclick="resetView()">Reset View</button>
  </div>
  <div id="tree-container">
    <svg id="tree-svg" width="__SVG_WIDTH__" height="__SVG_HEIGHT__"></svg>
  </div>

  <script src="https://d3js.org/d3.v7.min.js"></script>
  <script>
    const initialJsonData = __DATA_JSON__;

    function transformData(jsonData) {
        function processNode(node, parentL1StageName) {
            let displayName = node.name;
            if (node.total_count !== undefined) {
                if (!/\(Total Count: \d+\)$/.test(displayName)) {
                    displayName += ` (Total Count: ${node.total_count})`;
                }
            }
            const newNode = { name: displayName };
            if (parentL1StageName === "Root") {
                 newNode.originalStageName = node.name;
            } else {
                newNode.originalStageName = parentL1StageName;
            }
            if (node.children && node.children.length > 0) {
                const stageNameToPass = (parentL1StageName === "Root") ? node.name : parentL1StageName;
                newNode.children = node.children.map(child => processNode(child, stageNameToPass));
            }
            return newNode;
        }
        let rootDisplayName = jsonData.name;
        if (jsonData.total_count !== undefined && !/\(Total Count: \d+\)$/.test(rootDisplayName)) {
            rootDisplayName += ` (Total Count: ${jsonData.total_count})`;
        }
        return {
            name: rootDisplayName,
            originalStageName: "Root",
            children: (jsonData.children || []).map(child => processNode(child, "Root"))
        };
    }

    const treeData = transformData(initialJsonData);

    const PALETTE = [
      ["#3498DB","#2980B9","#AED6F1"], ["#2ECC71","#27AE60","#A9DFBF"],
      ["#E74C3C","#C0392B","#F5B7B1"], ["#9B59B6","#8E44AD","#D7BDE2"],
      ["#F39C12","#D68910","#FAD7A0"], ["#1ABC9C","#117864","#A2D9CE"],
      ["#34495E","#1B2631","#ABB2B9"], ["#E67E22","#BA4A00","#F5CBA7"],
      ["#16A085","#0E6655","#A2D9CE"], ["#D35400","#A04000","#EDBB99"],
      ["#7F8C8D","#566573","#D5DBDB"], ["#C0392B","#7B241C","#F5B7B1"],
      ["#2E86C1","#1B4F72","#A9CCE3"], ["#28B463","#196F3D","#A9DFBF"],
      ["#AF7AC5","#6C3483","#D2B4DE"],
    ];
    const phaseColors = { "Root": { fill: "#4A4A4A", stroke: "#333333", collapsedFill: "#6C757D" },
                          "Default": { fill: "#BDC3C7", stroke: "#95A5A6", collapsedFill: "#ECF0F1" } };
    (initialJsonData.children || []).forEach((c, i) => {
      const pal = PALETTE[i % PALETTE.length];
      phaseColors[c.name] = { fill: pal[0], stroke: pal[1], collapsedFill: pal[2] };
    });

    const levelSpecificPalettes = {
      0: { fill: "#4A4A4A", stroke: "#333333", collapsedFill: "#6C757D" },
      2: { fill: "#6ab04c", stroke: "#508a38", collapsedFill: "#a3d391" },
      3: { fill: "#f0932b", stroke: "#d0730f", collapsedFill: "#f6c07e" },
      4: { fill: "#be2edd", stroke: "#a01cb3", collapsedFill: "#e08bf2" },
      5: { fill: "#00a8ff", stroke: "#007ac1", collapsedFill: "#74d2ff" },
      6: { fill: "#e55039", stroke: "#c23620", collapsedFill: "#f09a8d" },
      default: { fill: "#747d8c", stroke: "#57606f", collapsedFill: "#a4b0be" }
    };

    const svgElement = d3.select("#tree-svg");
    const initialSvgWidth = +svgElement.attr("width");
    const initialSvgHeight = +svgElement.attr("height");
    const margin = { top: 40, right: 120, bottom: 80, left: 450 };
    let width = initialSvgWidth - margin.left - margin.right;
    let height = initialSvgHeight - margin.top - margin.bottom;
    const duration = 500;
    let nodeCounter = 0;
    const g = svgElement.append("g").attr("transform", `translate(${margin.left},${margin.top})`);
    const treemap = d3.tree().nodeSize([40, 0]);
    let rootNode = d3.hierarchy(treeData, d => d.children);
    rootNode.x0 = 0;
    rootNode.y0 = 0;

    if (rootNode.children) {
      rootNode.children.forEach(d_child => {
        if (d_child.children) { collapseBranch(d_child); }
      });
    }
    updateTree(rootNode);

    function collapseBranch(d) { if (d.children) { d._children = d.children; d._children.forEach(collapseBranch); d.children = null; } }
    function expandBranch(d) { if (d._children) { d.children = d._children; d._children = null; } if (d.children) { d.children.forEach(expandBranch); } }
    window.expandAll = () => { expandBranch(rootNode); updateTree(rootNode); };
    window.collapseAll = () => { if (rootNode.children) { rootNode.children.forEach(collapseBranch); } updateTree(rootNode); };
    window.resetView = () => { if (rootNode.children) { rootNode.children.forEach(d_child => { if (d_child.children || d_child._children) { collapseBranch(d_child); } }); } if (rootNode._children && !rootNode.children) { rootNode.children = rootNode._children; rootNode._children = null; } updateTree(rootNode); };

    function updateTree(source) {
      const treeLayoutData = treemap(rootNode);
      let nodes = treeLayoutData.descendants();
      let links = treeLayoutData.descendants().slice(1);

      let minX = 0;
      let maxX = 0;
      if (nodes.length > 0) {
        minX = d3.min(nodes, d => d.x);
        maxX = d3.max(nodes, d => d.x);
      }

      let neededHeight = Math.max(initialSvgHeight, maxX - minX + margin.top + margin.bottom + 100);
      svgElement.transition().duration(duration / 2).attr("height", neededHeight);
      g.transition().duration(duration / 2).attr("transform", `translate(${margin.left},${margin.top - minX + 40})`);

      nodes.forEach(d => { d.y = d.depth * 400; });

      const node = g.selectAll('g.node').data(nodes, d => d.id || (d.id = ++nodeCounter));
      const nodeEnter = node.enter().append('g')
        .attr('class', d => "node" + (d.children || d._children ? " node--internal" : " node--leaf") + (d._children ? " _children" : ""))
        .attr('transform', d => `translate(${source.y0},${source.x0})`)
        .on('click', (event, d) => { if (d.children) { d._children = d.children; d.children = null; } else if (d._children) { d.children = d._children; d._children = null; } updateTree(d); })
        .style('cursor', d => (d.children || d._children) ? 'pointer' : 'default');

      nodeEnter.append('circle').attr('r', 1e-6);

      nodeEnter.append('text')
        .attr('dy', '.35em')
        .attr('x', d => d.children || d._children ? -14 : 14)
        .attr('text-anchor', d => d.children || d._children ? 'end' : 'start')
        .style("fill-opacity", 1e-6)
        .call(wrapText, 380);

      const nodeUpdate = nodeEnter.merge(node);
      nodeUpdate.transition().duration(duration)
        .attr('transform', d => `translate(${d.y},${d.x})`)
        .attr('class', d => "node" + (d.children ? " node--internal" : " node--leaf") + (d._children ? " node--internal _children" : ""));

      nodeUpdate.select('circle').attr('r', 8.5)
        .style('fill', d => {
            let palette;
            if (d.depth === 0) {
                palette = levelSpecificPalettes[0];
            } else if (d.depth === 1) {
                palette = phaseColors[d.data.originalStageName] || phaseColors.Default;
            } else {
                palette = levelSpecificPalettes[d.depth] || levelSpecificPalettes.default;
            }
            if (d._children) return palette.collapsedFill;
            if (d.children) return palette.fill;
            return "#fff";
        })
        .style('stroke', d => {
            let palette;
            if (d.depth === 0) {
                palette = levelSpecificPalettes[0];
            } else if (d.depth === 1) {
                palette = phaseColors[d.data.originalStageName] || phaseColors.Default;
            } else {
                palette = levelSpecificPalettes[d.depth] || levelSpecificPalettes.default;
            }
            return palette.stroke;
        });
      nodeUpdate.select('text').style("fill-opacity", 1).call(wrapText, 380);

      const nodeExit = node.exit().transition().duration(duration).attr('transform', d => `translate(${source.y},${source.x})`).remove();
      nodeExit.select('circle').attr('r', 1e-6);
      nodeExit.select('text').style('fill-opacity', 1e-6);

      const link = g.selectAll('path.link').data(links, d => d.id);
      const linkEnter = link.enter().insert('path', "g").attr('class', 'link').attr('d', d => { const o = { x: source.x0, y: source.y0 }; return diagonal(o, o); });

      linkEnter.merge(link).transition().duration(duration).attr('d', d => diagonal(d, d.parent))
        .style('stroke', d => {
            const sourceNode = d.parent;
            if (!sourceNode) return phaseColors.Default.stroke;
            const l1AncestorName = sourceNode.data.originalStageName;
            const colorPalette = phaseColors[l1AncestorName] || phaseColors.Default;
            return colorPalette.stroke;
        });
      link.exit().transition().duration(duration).attr('d', d => { const o = { x: source.x, y: source.y }; return diagonal(o, o); }).remove();
      nodes.forEach(d => { d.x0 = d.x; d.y0 = d.y; });
    }

    function diagonal(s, d) { return `M ${s.y} ${s.x} C ${(s.y + d.y) / 2} ${s.x}, ${(s.y + d.y) / 2} ${d.x}, ${d.y} ${d.x}`; }

    function wrapText(textElements, maxWidth) {
        const textPartColors = { name: '#343a40', count: '#0056b3' };
        const countRegex = /(\s\(Total Count: \d+\))$/;

        textElements.each(function () {
            const textD3 = d3.select(this);
            const originalNodeText = textD3.datum().data.name;
            const x = parseFloat(textD3.attr("x") || 0);
            const initialDy = textD3.attr("dy");
            const textAnchor = textD3.attr("text-anchor");
            const lineHeight = 1.1;

            textD3.text(null);

            let namePart = originalNodeText;
            let countPartText = "";

            const countMatch = originalNodeText.match(countRegex);
            if (countMatch && originalNodeText.endsWith(countMatch[0])) {
                namePart = originalNodeText.substring(0, originalNodeText.length - countMatch[0].length).trim();
                countPartText = countMatch[0].trim();
            }

            const tokens = [];
            namePart.split(/\s+/).filter(Boolean).forEach(word => {
                tokens.push({ text: word, type: 'name' });
            });
            if (countPartText) {
                tokens.push({ text: countPartText, type: 'count' });
            }

            if (tokens.length === 0 && originalNodeText) {
                tokens.push({ text: originalNodeText, type: 'name' });
            }

            let currentTspan = textD3.append("tspan").attr("x", x).attr("dy", initialDy);
            if (textAnchor === "end") currentTspan.attr("text-anchor", "end");

            let lineTokens = [];

            for (let i = 0; i < tokens.length; i++) {
                const tokenObj = tokens[i];
                lineTokens.push(tokenObj);
                currentTspan.text(lineTokens.map(t => t.text).join(" "));
                if (currentTspan.node().getComputedTextLength() > maxWidth && lineTokens.length > 1) {
                    lineTokens.pop();
                    currentTspan.text(null);
                    lineTokens.forEach((prevToken, idx) => {
                        currentTspan.append("tspan")
                            .text((idx > 0 ? " " : "") + prevToken.text)
                            .style("fill", textPartColors[prevToken.type] || textPartColors.name)
                            .style("font-weight", prevToken.type === 'count' ? "bold" : "normal");
                    });
                    lineTokens = [tokenObj];
                    currentTspan = textD3.append("tspan").attr("x", x).attr("dy", lineHeight + "em");
                    if (textAnchor === "end") currentTspan.attr("text-anchor", "end");
                }
            }

            currentTspan.text(null);
            lineTokens.forEach((token, idx) => {
                currentTspan.append("tspan")
                    .text((idx > 0 ? " " : "") + token.text)
                    .style("fill", textPartColors[token.type] || textPartColors.name)
                    .style("font-weight", token.type === 'count' ? "bold" : "normal");
            });

            if (textD3.selectAll("tspan > tspan").empty() && textD3.select("tspan").text().length === 0 && originalNodeText) {
                let t = textD3.select("tspan");
                let displayText = originalNodeText;
                t.text(displayText).style("fill", textPartColors.name);
                if (t.node() && t.node().getComputedTextLength() > maxWidth && displayText.length > 20) {
                    let estimatedChars = Math.floor(maxWidth / (t.node().getComputedTextLength()/displayText.length) );
                    displayText = displayText.substring(0, Math.max(0, estimatedChars - 3)) + "...";
                    t.text(displayText);
                }
            }
        });
    }
  </script>
</body>
</html>
"##;

pub fn emit_html(
    tree: &Value,
    title: &str,
    header: &str,
    svg_width: u32,
    svg_height: u32,
) -> String {
    let data_json = serde_json::to_string(tree)
        .unwrap_or_else(|_| "{}".to_string())
        .replace("</", "<\\/");

    HTML_TEMPLATE
        .replace("__TITLE__", &html_escape(title))
        .replace("__HEADER__", &html_escape(header))
        .replace("__SVG_WIDTH__", &svg_width.to_string())
        .replace("__SVG_HEIGHT__", &svg_height.to_string())
        .replace("__DATA_JSON__", &data_json)
}

pub fn write_tree_html(
    graph_path: &Path,
    output_path: &Path,
    root: Option<&str>,
    max_children: usize,
    project_label: Option<&str>,
) -> Result<PathBuf> {
    check_file_size(graph_path, MAX_GRAPH_FILE_BYTES)?;
    let raw = std::fs::read_to_string(graph_path)?;
    let graph: Value = serde_json::from_str(&raw)?;

    let tree = build_tree(&graph, root, max_children, project_label);
    let tree_name = tree.get("name").and_then(|v| v.as_str()).unwrap_or("graph");
    let title = format!("{} \u{2014} codesynapse tree viewer", tree_name);
    let header = format!("{} \u{2014} Knowledge Graph", tree_name);
    let html = emit_html(&tree, &title, &header, 6000, 8000);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, html)?;
    Ok(output_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_common_root_empty() {
        assert_eq!(common_root(&[]), "");
    }

    #[test]
    fn test_common_root_single() {
        let r = common_root(&["/home/user/project/src/main.py"]);
        assert!(r.contains("home") || r.contains("project") || !r.is_empty());
    }

    #[test]
    fn test_common_root_multiple_same_dir() {
        let r = common_root(&["/home/user/proj/a.py", "/home/user/proj/b.py"]);
        assert!(r.ends_with("proj") || r.contains("proj"), "got: {r}");
    }

    #[test]
    fn test_common_root_no_common() {
        let r = common_root(&["/a/b/c.py", "/x/y/z.py"]);
        assert!(r == "/" || r.is_empty() || r == "\\");
    }

    #[test]
    fn test_common_root_filter_empty() {
        let r = common_root(&["", "/home/user/proj/main.py"]);
        assert!(!r.is_empty());
    }

    #[test]
    fn test_make_truncation_leaf() {
        let leaf = make_truncation_leaf(42);
        assert_eq!(leaf["name"].as_str().unwrap(), "(+42 more)");
        assert_eq!(leaf["total_count"].as_u64().unwrap(), 42);
        assert!(leaf["children"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_build_tree_empty_graph() {
        let graph = json!({"nodes": []});
        let tree = build_tree(&graph, None, 200, None);
        assert_eq!(tree["name"].as_str().unwrap(), "(empty graph)");
        assert_eq!(tree["total_count"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_build_tree_no_nodes_key() {
        let graph = json!({});
        let tree = build_tree(&graph, None, 200, None);
        assert_eq!(tree["name"].as_str().unwrap(), "(empty graph)");
    }

    #[test]
    fn test_build_tree_basic() {
        let graph = json!({
            "nodes": [
                {"id": "a", "label": "MyClass", "source_file": "/proj/src/main.py", "file_type": "code"},
                {"id": "b", "label": "helper", "source_file": "/proj/src/util.py", "file_type": "code"}
            ]
        });
        let tree = build_tree(&graph, Some("/proj"), 200, None);
        let children = tree["children"].as_array().unwrap();
        assert!(!children.is_empty(), "tree should have children");
        assert!(tree["total_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_build_tree_project_label() {
        let graph = json!({
            "nodes": [
                {"id": "x", "label": "Foo", "source_file": "/proj/a.py", "file_type": "code"}
            ]
        });
        let tree = build_tree(&graph, Some("/proj"), 200, Some("MyProject"));
        assert_eq!(tree["name"].as_str().unwrap(), "MyProject");
    }

    #[test]
    fn test_build_tree_max_children_truncation() {
        let syms: Vec<Value> = (0..5).map(|i| {
            json!({"id": format!("n{i}"), "label": format!("Sym{i}"), "source_file": "/proj/big.py", "file_type": "code"})
        }).collect();
        let graph = json!({"nodes": syms});
        let tree = build_tree(&graph, Some("/proj"), 3, None);

        fn find_truncation(node: &Value) -> bool {
            if let Some(name) = node["name"].as_str() {
                if name.starts_with("(+") {
                    return true;
                }
            }
            if let Some(kids) = node["children"].as_array() {
                for k in kids {
                    if find_truncation(k) {
                        return true;
                    }
                }
            }
            false
        }
        assert!(find_truncation(&tree), "should have truncation leaf");
    }

    #[test]
    fn test_emit_html_contains_title() {
        let tree = json!({"name": "myproject", "total_count": 1, "children": []});
        let html = emit_html(&tree, "My Title", "My Header", 6000, 8000);
        assert!(html.contains("<title>My Title</title>"));
        assert!(html.contains("My Header"));
        assert!(!html.contains("__DATA_JSON__"));
    }

    #[test]
    fn test_emit_html_script_injection_escaped() {
        let tree = json!({"name": "x</script><script>alert(1)", "total_count": 1, "children": []});
        let html = emit_html(&tree, "t", "h", 100, 100);
        assert!(
            !html.contains("</script><script>alert(1)"),
            "raw </script> must not appear unescaped"
        );
    }

    #[test]
    fn test_emit_html_svg_dimensions() {
        let tree = json!({"name": "r", "total_count": 0, "children": []});
        let html = emit_html(&tree, "t", "h", 1234, 5678);
        assert!(html.contains("width=\"1234\""));
        assert!(html.contains("height=\"5678\""));
    }
}
