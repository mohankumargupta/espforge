use anyhow::{Result, anyhow};
use ruchy::backend::Transpiler;
use ruchy::frontend::Parser;
use ruchy::frontend::ast::{ExprKind, TypeKind};

pub struct RuchyOutput {
    pub setup: String,
    pub loop_body: String,
    pub variables: Vec<String>,
    pub task_definitions: Vec<String>,
    pub task_names: Vec<String>,
    pub task_spawns: Vec<String>, // New field to hold full spawn calls
}

pub fn compile_ruchy_script(raw_source: &str, enable_async: bool) -> Result<RuchyOutput> {
    // Change Windows (CRLF) line endings to Unix (LF).
    let source = raw_source.replace("\r\n", "\n");

    let mut parser = Parser::new(&source);
    let ast = parser
        .parse()
        .map_err(|e| anyhow!("Failed to parse Ruchy code: {:?}", e))?;
    let mut transpiler = Transpiler::new();

    let mut setup_body = String::new();
    let mut loop_body = String::new();
    let mut variables = Vec::new();
    let mut task_definitions = Vec::new();
    let mut task_names = Vec::new();
    let mut task_spawns = Vec::new();

    if let ExprKind::Block(exprs) = ast.kind {
        for expr in exprs {
            let is_disabled = expr.attributes.iter().any(|attr| attr.name == "disabled");
            if is_disabled {
                continue;
            }
            let has_task_attr = expr.attributes.iter().any(|attr| attr.name == "task");
            
            match expr.kind {
                ExprKind::Function { name, body, params, .. } => {
                    let is_task = has_task_attr || name.starts_with("task_");

                    if let ExprKind::Block(ref stmts) = body.kind {
                        transpiler.analyze_mutability(stmts);
                    } else {
                        transpiler.analyze_mutability(&[body.as_ref().clone()]);
                    }
                    let token_stream = transpiler.transpile_expr(&body)?;
                    let raw_code = token_stream.to_string();

                    let indent = if is_task || name == "forever" { 2 } else { 1 };
                    let formatted_body = format_rust_code(&raw_code, indent, enable_async);

                    if is_task {
                        if !enable_async {
                            return Err(anyhow!(
                                "Function '{}' identified as task but 'enable_async' is false in configuration.",
                                name
                            ));
                        }

                        task_names.push(name.clone());

                        // Build parameter string for the function definition: "arg1: Type1, arg2: Type2"
                        let mut fn_params = Vec::new();
                        // Build arguments string for the spawn call: "arg1, arg2"
                        let mut call_args = Vec::new();

                        for param in &params {
                            let param_name = param.name();
                            let type_name = match &param.ty.kind {
                                TypeKind::Named(n) => n.clone(),
                                _ => "i32".to_string(), // Default fallback
                            };
                            
                            // For components like Button, we typically pass them by move or mutable reference
                            // In Embassy, moving the peripheral driver into the task is standard.
                            fn_params.push(format!("mut {}: {}", param_name, type_name));
                            call_args.push(param_name);
                        }
                        
                        let fn_params_str = fn_params.join(", ");
                        let call_args_str = call_args.join(", ");

                        // Wrap the body in an infinite loop and the embassy task macro
                        let task_code = format!(
                             "#[embassy_executor::task]\nasync fn {}({}) {{\n{}\n}}",
                            name, fn_params_str, formatted_body
                        );
                        task_definitions.push(task_code);
                        
                        // Generate the spawn call
                        task_spawns.push(format!("spawner.spawn({}({})).ok();", name, call_args_str));

                    } else {
                        match name.as_str() {
                            "setup" => {
                                setup_body = formatted_body;
                            }
                            "forever" => {
                                loop_body = formatted_body;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {
                    transpiler.analyze_mutability(std::slice::from_ref(&expr));
                    let token_stream = transpiler.transpile_expr(&expr)?;
                    let raw_code = token_stream.to_string();

                    let mut stmt = raw_code.trim().to_string();
                    if !stmt.ends_with(';') {
                        stmt.push(';');
                    }
                    if stmt.starts_with("let ") {
                        if !stmt.starts_with("let mut ") {
                            stmt = stmt.replace("let ", "let mut ");
                        }
                    } else {
                        stmt = format!("let mut {}", stmt);
                    }
                    variables.push(stmt);
                }
            }
        }
    }

    Ok(RuchyOutput {
        setup: setup_body,
        loop_body,
        variables,
        task_definitions,
        task_names,
        task_spawns,
    })
}

fn apply_async_delay_replacement(input: String) -> String {
    let search = "delay.delay_millis(";
    let replacement_start = "Timer::after(Duration::from_millis(";
    
    let mut result = String::with_capacity(input.len());
    let mut last_pos = 0;
    
    while let Some(start_idx) = input[last_pos..].find(search) {
        let abs_start = last_pos + start_idx;
        result.push_str(&input[last_pos..abs_start]);
        result.push_str(replacement_start);
        
        let args_start = abs_start + search.len();
        let mut paren_balance = 1;
        let mut end_idx = args_start;
        
        for (i, c) in input[args_start..].char_indices() {
            if c == '(' { paren_balance += 1; }
            else if c == ')' { paren_balance -= 1; }
            
            if paren_balance == 0 {
                end_idx = args_start + i;
                break;
            }
        }
        
        result.push_str(&input[args_start..end_idx]);
        result.push_str(")).await");
        last_pos = end_idx + 1; 
    }
    
    result.push_str(&input[last_pos..]);
    result
}

fn format_rust_code(input: &str, indent_level: usize, enable_async: bool) -> String {
    let indent = "    ".repeat(indent_level);
    let mut content = input.trim();
    while content.starts_with('{') && content.ends_with('}') {
        content = content[1..content.len() - 1].trim();
    }

    // Fix token spacing artifacts
    let mut cleaned = content.to_string();
    cleaned = cleaned.replace(" . ", ".");
    cleaned = cleaned.replace(" (", "(");
    cleaned = cleaned.replace(" )", ")");
    cleaned = cleaned.replace(" ;", ";");
    cleaned = cleaned.replace(" !", "!");
    cleaned = cleaned.replace(" :: ", "::");

    if enable_async {
        cleaned = apply_async_delay_replacement(cleaned);
    }

    let mut formatted = String::new();
    let statements: Vec<&str> = cleaned
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for (i, stmt) in statements.iter().enumerate() {
        let  final_stmt = stmt.to_string();
        
        // Remove extra braces if present (e.g., from nested loops transpiled as blocks)
        if final_stmt.starts_with("{") && final_stmt.ends_with("}") {
             // This is a heuristic to clean up simple double wrapping
             // Keep content but remove outer braces if it's just a simple block wrapper
             // However, for loops like `loop { ... }`, we want to keep them if they are semantic.
             // But here, the task wrapper provides the loop.
        }

        if i == 0 {
            formatted.push_str(&format!("{};", final_stmt));
        } else {
            formatted.push_str(&format!("\n{}{};", indent, final_stmt));
        }
    }

    formatted
}

