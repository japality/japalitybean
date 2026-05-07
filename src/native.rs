use crate::ast::*;
use std::collections::HashMap;

const BASE_ADDR: u64 = 0x400000;
const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const CODE_OFFSET: usize = ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE;

#[derive(Clone)]
struct LabelPatch {
    pos: usize,
    label: String,
}

#[derive(Clone)]
struct Abs64Patch {
    pos: usize,
    label: String,
}

#[derive(Clone)]
struct Slot {
    offset: i32,
    ty: Type,
}

#[derive(Clone)]
struct LoopLabels {
    latch: String,
    exit: String,
}

#[derive(Clone, Copy)]
struct ArgReg {
    xor_rm: u8,
    store_modrm: u8,
    mov_from_eax_modrm: u8,
}

const ARG_REGS: [ArgReg; 4] = [
    ArgReg {
        xor_rm: 0xff,              // xor edi, edi
        store_modrm: 0xbd,         // mov [rbp+disp32], edi
        mov_from_eax_modrm: 0xc7,  // mov edi, eax
    },
    ArgReg {
        xor_rm: 0xf6,              // xor esi, esi
        store_modrm: 0xb5,         // mov [rbp+disp32], esi
        mov_from_eax_modrm: 0xc6,  // mov esi, eax
    },
    ArgReg {
        xor_rm: 0xd2,              // xor edx, edx
        store_modrm: 0x95,         // mov [rbp+disp32], edx
        mov_from_eax_modrm: 0xc2,  // mov edx, eax
    },
    ArgReg {
        xor_rm: 0xc9,              // xor ecx, ecx
        store_modrm: 0x8d,         // mov [rbp+disp32], ecx
        mov_from_eax_modrm: 0xc1,  // mov ecx, eax
    },
];

pub fn emit_linux_x86_64_executable(program: &Program, entry: &str) -> Result<Vec<u8>, String> {
    let mut module = NativeModule::new(program, entry)?;
    module.emit()?;
    Ok(wrap_elf(module.code))
}

struct NativeModule<'a> {
    program: &'a Program,
    entry: String,
    code: Vec<u8>,
    labels: HashMap<String, usize>,
    patches: Vec<LabelPatch>,
    abs64_patches: Vec<Abs64Patch>,
    data_items: Vec<(String, Vec<u8>)>,
    unique_counter: usize,
}

impl<'a> NativeModule<'a> {
    fn new(program: &'a Program, entry: &str) -> Result<Self, String> {
        if !program.functions.iter().any(|func| func.kind.name == entry) {
            return Err(format!("native entrypoint function not found: {entry}"));
        }
        Ok(Self {
            program,
            entry: entry.to_string(),
            code: Vec::new(),
            labels: HashMap::new(),
            patches: Vec::new(),
            abs64_patches: Vec::new(),
            data_items: Vec::new(),
            unique_counter: 0,
        })
    }

    fn emit(&mut self) -> Result<(), String> {
        self.emit_start_stub()?;
        self.emit_parse_i32_helper();
        self.emit_debug_i32_helper();
        for func in &self.program.functions {
            FunctionEmitter::new(self, &func.kind).emit()?;
        }
        self.emit_static_data();
        self.patch_rel32()?;
        self.patch_abs64()?;
        Ok(())
    }

    fn emit_start_stub(&mut self) -> Result<(), String> {
        let entry_func = self
            .program
            .functions
            .iter()
            .find(|func| func.kind.name == self.entry)
            .expect("entry checked in constructor");
        self.emit_entry_args(&entry_func.kind.inputs);
        self.emit_call(&func_label(&self.entry));
        self.emit_bytes(&[0x89, 0xc7]); // mov edi, eax
        self.emit_bytes(&[0xb8]);
        self.emit_i32(60); // mov eax, SYS_exit
        self.emit_bytes(&[0x0f, 0x05]); // syscall
        Ok(())
    }

    fn emit_entry_args(&mut self, inputs: &[StickyBinding]) {
        for (idx, input) in inputs.iter().enumerate().take(ARG_REGS.len()) {
            self.emit_bytes(&[0x31, ARG_REGS[idx].xor_rm]);
            if is_native_int(&input.ty) {
                let missing = self.fresh_label("argv.missing");
                self.emit_bytes(&[0x48, 0x8b, 0xbc, 0x24]);
                self.emit_i32(16 + (idx as i32 * 8)); // argv[idx + 1]
                self.emit_bytes(&[0x48, 0x85, 0xff]); // test rdi, rdi
                self.emit_jcc(0x84, &missing); // je missing
                self.emit_call("runtime.parse_i32");
                self.emit_bytes(&[0x89, ARG_REGS[idx].mov_from_eax_modrm]); // mov arg_reg, eax
                self.mark_label(&missing);
            }
        }
    }

    fn emit_parse_i32_helper(&mut self) {
        self.mark_label("runtime.parse_i32");
        self.emit_bytes(&[0x31, 0xc0]); // xor eax, eax
        self.emit_bytes(&[0x31, 0xc9]); // xor ecx, ecx
        self.emit_bytes(&[0x0f, 0xb6, 0x17]); // movzx edx, byte ptr [rdi]
        self.emit_bytes(&[0x80, 0xfa, 0x2d]); // cmp dl, '-'
        let digits = self.fresh_label("parse.digits");
        self.emit_jcc(0x85, &digits); // jne digits
        self.emit_bytes(&[0xb9]);
        self.emit_i32(1); // mov ecx, 1
        self.emit_bytes(&[0x48, 0xff, 0xc7]); // inc rdi
        self.mark_label(&digits);

        let loop_label = self.fresh_label("parse.loop");
        let done = self.fresh_label("parse.done");
        let positive = self.fresh_label("parse.positive");
        self.mark_label(&loop_label);
        self.emit_bytes(&[0x0f, 0xb6, 0x17]); // movzx edx, byte ptr [rdi]
        self.emit_bytes(&[0x80, 0xfa, 0x30]); // cmp dl, '0'
        self.emit_jcc(0x82, &done); // jb done
        self.emit_bytes(&[0x80, 0xfa, 0x39]); // cmp dl, '9'
        self.emit_jcc(0x87, &done); // ja done
        self.emit_bytes(&[0x69, 0xc0]);
        self.emit_i32(10); // imul eax, eax, 10
        self.emit_bytes(&[0x83, 0xea, 0x30]); // sub edx, '0'
        self.emit_bytes(&[0x01, 0xd0]); // add eax, edx
        self.emit_bytes(&[0x48, 0xff, 0xc7]); // inc rdi
        self.emit_jmp(&loop_label);
        self.mark_label(&done);
        self.emit_bytes(&[0x85, 0xc9]); // test ecx, ecx
        self.emit_jcc(0x84, &positive); // je positive
        self.emit_bytes(&[0xf7, 0xd8]); // neg eax
        self.mark_label(&positive);
        self.emit_bytes(&[0xc3]); // ret
    }

    fn emit_debug_i32_helper(&mut self) {
        self.mark_label("runtime.debug_i32");
        self.emit_bytes(&[0x55]); // push rbp
        self.emit_bytes(&[0x48, 0x89, 0xe5]); // mov rbp, rsp
        self.emit_bytes(&[0x53]); // push rbx
        self.emit_bytes(&[0x48, 0x83, 0xec, 0x20]); // sub rsp, 32
        self.emit_bytes(&[0x89, 0xf8]); // mov eax, edi
        self.emit_bytes(&[0x48, 0x8d, 0x75, 0xff]); // lea rsi, [rbp-1]
        self.emit_bytes(&[0xc6, 0x06, 0x0a]); // mov byte [rsi], '\n'
        self.emit_bytes(&[0xb9]);
        self.emit_i32(1); // mov ecx, 1
        self.emit_bytes(&[0x83, 0xf8, 0x00]); // cmp eax, 0
        let loop_label = self.fresh_label("debug.loop");
        let write = self.fresh_label("debug.write");
        self.emit_jcc(0x85, &loop_label); // jne loop
        self.emit_bytes(&[0x48, 0xff, 0xce]); // dec rsi
        self.emit_bytes(&[0xc6, 0x06, 0x30]); // mov byte [rsi], '0'
        self.emit_bytes(&[0xff, 0xc1]); // inc ecx
        self.emit_jmp(&write);
        self.mark_label(&loop_label);
        self.emit_bytes(&[0x31, 0xd2]); // xor edx, edx
        self.emit_bytes(&[0xbb]);
        self.emit_i32(10); // mov ebx, 10
        self.emit_bytes(&[0xf7, 0xf3]); // div ebx
        self.emit_bytes(&[0x80, 0xc2, 0x30]); // add dl, '0'
        self.emit_bytes(&[0x48, 0xff, 0xce]); // dec rsi
        self.emit_bytes(&[0x88, 0x16]); // mov [rsi], dl
        self.emit_bytes(&[0xff, 0xc1]); // inc ecx
        self.emit_bytes(&[0x85, 0xc0]); // test eax, eax
        self.emit_jcc(0x85, &loop_label); // jne loop
        self.mark_label(&write);
        self.emit_bytes(&[0xb8]);
        self.emit_i32(1); // mov eax, SYS_write
        self.emit_bytes(&[0xbf]);
        self.emit_i32(1); // mov edi, stdout
        self.emit_bytes(&[0x89, 0xca]); // mov edx, ecx
        self.emit_bytes(&[0x0f, 0x05]); // syscall
        self.emit_bytes(&[0x48, 0x83, 0xc4, 0x20]); // add rsp, 32
        self.emit_bytes(&[0x5b]); // pop rbx
        self.emit_bytes(&[0x5d]); // pop rbp
        self.emit_bytes(&[0xc3]); // ret
    }

    fn mark_label(&mut self, label: &str) {
        self.labels.insert(label.to_string(), self.code.len());
    }

    fn emit_call(&mut self, label: &str) {
        self.emit_bytes(&[0xe8]);
        self.emit_rel32_patch(label);
    }

    fn emit_jmp(&mut self, label: &str) {
        self.emit_bytes(&[0xe9]);
        self.emit_rel32_patch(label);
    }

    fn emit_jcc(&mut self, opcode: u8, label: &str) {
        self.emit_bytes(&[0x0f, opcode]);
        self.emit_rel32_patch(label);
    }

    fn emit_rel32_patch(&mut self, label: &str) {
        let pos = self.code.len();
        self.patches.push(LabelPatch {
            pos,
            label: label.to_string(),
        });
        self.emit_i32(0);
    }

    fn emit_abs64_patch(&mut self, label: &str) {
        let pos = self.code.len();
        self.abs64_patches.push(Abs64Patch {
            pos,
            label: label.to_string(),
        });
        self.code.extend_from_slice(&0u64.to_le_bytes());
    }

    fn add_static_bytes(&mut self, prefix: &str, bytes: Vec<u8>) -> String {
        let label = self.fresh_label(prefix);
        self.data_items.push((label.clone(), bytes));
        label
    }

    fn emit_static_data(&mut self) {
        let data_items = std::mem::take(&mut self.data_items);
        for (label, bytes) in data_items {
            self.mark_label(&label);
            self.emit_bytes(&bytes);
        }
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        self.unique_counter += 1;
        format!("{}.{}", sanitize(prefix), self.unique_counter)
    }

    fn patch_rel32(&mut self) -> Result<(), String> {
        for patch in &self.patches {
            let Some(target) = self.labels.get(&patch.label).copied() else {
                return Err(format!("native backend unresolved label: {}", patch.label));
            };
            let next = patch.pos + 4;
            let rel = target as i64 - next as i64;
            let rel = i32::try_from(rel)
                .map_err(|_| format!("native backend branch is out of range: {}", patch.label))?;
            self.code[patch.pos..patch.pos + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Ok(())
    }

    fn patch_abs64(&mut self) -> Result<(), String> {
        for patch in &self.abs64_patches {
            let Some(target) = self.labels.get(&patch.label).copied() else {
                return Err(format!("native backend unresolved data label: {}", patch.label));
            };
            let addr = BASE_ADDR + CODE_OFFSET as u64 + target as u64;
            self.code[patch.pos..patch.pos + 8].copy_from_slice(&addr.to_le_bytes());
        }
        Ok(())
    }

    fn emit_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn emit_i32(&mut self, value: i32) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }
}

struct FunctionEmitter<'m, 'a> {
    module: &'m mut NativeModule<'a>,
    func: &'a FunctionDecl,
    locals: HashMap<String, Slot>,
    next_offset: i32,
    returned: bool,
    loops: Vec<LoopLabels>,
}

impl<'m, 'a> FunctionEmitter<'m, 'a> {
    fn new(module: &'m mut NativeModule<'a>, func: &'a FunctionDecl) -> Self {
        Self {
            module,
            func,
            locals: HashMap::new(),
            next_offset: 0,
            returned: false,
            loops: Vec::new(),
        }
    }

    fn emit(mut self) -> Result<(), String> {
        self.collect_function_slots();
        let stack_size = align_to(self.next_offset, 16);

        self.module.mark_label(&func_label(&self.func.name));
        self.emit_bytes(&[0x55]); // push rbp
        self.emit_bytes(&[0x48, 0x89, 0xe5]); // mov rbp, rsp
        self.emit_bytes(&[0x48, 0x81, 0xec]);
        self.emit_i32(stack_size); // sub rsp, stack_size

        self.zero_all_slots()?;
        self.store_input_args()?;

        for stmt in &self.func.body {
            if self.returned {
                break;
            }
            self.emit_stmt(stmt)?;
        }

        if !self.returned {
            self.emit_mov_eax_mem(&self.func.output.name)?;
            self.emit_return();
        }
        Ok(())
    }

    fn collect_function_slots(&mut self) {
        for input in &self.func.inputs {
            self.alloc_slot(&input.name, &input.ty);
        }
        self.alloc_slot(&self.func.output.name, &self.func.output.ty);
        for stmt in &self.func.body {
            self.collect_stmt_slots(stmt);
        }
    }

    fn collect_stmt_slots(&mut self, stmt: &Node<Statement>) {
        match &stmt.kind {
            Statement::Let(stmt) => {
                self.alloc_slot(&stmt.binding.name, &stmt.binding.ty);
                if is_vector_i32_3_call(&stmt.value) {
                    self.alloc_slot(&vec_data_name(&stmt.binding.name), &Type::Option(Box::new(Type::I32)));
                }
            }
            Statement::Loop(loop_block) => {
                self.alloc_slot(&loop_block.item.name, &loop_block.item.ty);
                self.alloc_slot(&loop_index_name(&stmt.id), &Type::I64);
                for child in &loop_block.body {
                    self.collect_stmt_slots(child);
                }
            }
            _ => {}
        }
    }

    fn alloc_slot(&mut self, name: &str, ty: &Type) {
        if self.locals.contains_key(name) {
            return;
        }
        self.next_offset += type_slot_size(ty);
        self.locals.insert(
            name.to_string(),
            Slot {
                offset: self.next_offset,
                ty: ty.clone(),
            },
        );
    }

    fn zero_all_slots(&mut self) -> Result<(), String> {
        let slots = self.locals.clone();
        for (name, slot) in slots {
            let words = type_slot_size(&slot.ty) / 8;
            for idx in 0..words {
                self.emit_mov_qword_mem_imm32_at(&name, idx * 8, 0)?;
            }
        }
        Ok(())
    }

    fn store_input_args(&mut self) -> Result<(), String> {
        let inputs = self.func.inputs.clone();
        for (idx, input) in inputs.iter().enumerate().take(ARG_REGS.len()) {
            if is_native_int(&input.ty) {
                let disp = self.disp(&input.name, 0)?;
                self.emit_bytes(&[0x89, ARG_REGS[idx].store_modrm]); // mov [rbp+disp32], reg32
                self.emit_i32(disp);
            }
        }
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Node<Statement>) -> Result<(), String> {
        match &stmt.kind {
            Statement::Let(stmt) => {
                if self.emit_vector_i32_3_let(stmt)? {
                    return Ok(());
                }
                self.emit_expr(&stmt.value)?;
                self.emit_mov_mem_eax(&stmt.binding.name)
            }
            Statement::Set(stmt) => {
                self.emit_expr(&stmt.value)?;
                self.emit_mov_mem_eax(&stmt.target)
            }
            Statement::Return(stmt) => {
                if let Some(value) = &stmt.value {
                    self.emit_expr(value)?;
                } else {
                    self.emit_bytes(&[0x31, 0xc0]); // xor eax, eax
                }
                self.emit_return();
                self.returned = true;
                Ok(())
            }
            Statement::Loop(loop_block) => self.emit_loop(stmt, loop_block),
            Statement::Condition(cond) => self.emit_condition(cond),
            Statement::Expr(expr) => {
                self.emit_expr(expr)?;
                Ok(())
            }
        }
    }

    fn emit_loop(&mut self, stmt: &Node<Statement>, loop_block: &LoopBlock) -> Result<(), String> {
        let collection = self
            .locals
            .get(&loop_block.collection)
            .cloned()
            .ok_or_else(|| format!("native backend could not resolve loop collection: {}", loop_block.collection))?;
        let Type::Vector(inner) = &collection.ty else {
            return Err("native backend loops currently require Vector<T>".to_string());
        };
        if !matches!(**inner, Type::I32 | Type::U32 | Type::Bool) {
            return Err("native backend loops currently support Vector<i32> items".to_string());
        }

        let idx_name = loop_index_name(&stmt.id);
        self.emit_mov_qword_mem_imm32_at(&idx_name, 0, 0)?;

        let header = self.module.fresh_label("loop.header");
        let body = self.module.fresh_label("loop.body");
        let latch = self.module.fresh_label("loop.latch");
        let exit = self.module.fresh_label("loop.exit");
        self.module.mark_label(&header);

        self.emit_mov_rax_mem_qword(&idx_name, 0)?;
        self.emit_bytes(&[0x48, 0x8b, 0x8d]); // mov rcx, qword [rbp+collection.len]
        self.emit_i32(-(collection.offset - 8));
        self.emit_bytes(&[0x48, 0x39, 0xc8]); // cmp rax, rcx
        self.module.emit_jcc(0x8d, &exit); // jge exit
        self.module.emit_jmp(&body);

        self.module.mark_label(&body);
        self.emit_bytes(&[0x48, 0x8b, 0x95]); // mov rdx, qword [rbp+collection.data]
        self.emit_i32(-collection.offset);
        self.emit_mov_rax_mem_qword(&idx_name, 0)?;
        self.emit_bytes(&[0x8b, 0x04, 0x82]); // mov eax, dword [rdx + rax*4]
        self.emit_mov_mem_eax(&loop_block.item.name)?;

        self.loops.push(LoopLabels {
            latch: latch.clone(),
            exit: exit.clone(),
        });
        self.returned = false;
        for child in &loop_block.body {
            if self.returned {
                break;
            }
            self.emit_stmt(child)?;
        }
        self.loops.pop();
        if !self.returned {
            self.module.emit_jmp(&latch);
        }
        self.returned = false;

        self.module.mark_label(&latch);
        self.emit_mov_rax_mem_qword(&idx_name, 0)?;
        self.emit_bytes(&[0x48, 0x83, 0xc0, 0x01]); // add rax, 1
        self.emit_mov_mem_rax_qword(&idx_name, 0)?;
        self.module.emit_jmp(&header);
        self.module.mark_label(&exit);
        Ok(())
    }

    fn emit_condition(&mut self, cond: &ConditionBlock) -> Result<(), String> {
        let next = self.module.fresh_label(&format!("cond.{}.next", cond.name));
        self.emit_expr(&cond.guard.kind.cond)?;
        self.emit_bytes(&[0x83, 0xf8, 0x00]); // cmp eax, 0
        self.module.emit_jcc(0x84, &next); // je next
        self.emit_guard_action(&cond.guard.kind.action, &next)?;
        self.module.mark_label(&next);
        self.returned = false;
        Ok(())
    }

    fn emit_guard_action(&mut self, action: &GuardAction, next: &str) -> Result<(), String> {
        match action {
            GuardAction::Continue => {
                let labels = self
                    .loops
                    .last()
                    .ok_or_else(|| "native backend found @continue outside loop".to_string())?;
                self.module.emit_jmp(&labels.latch);
            }
            GuardAction::Break => {
                let labels = self
                    .loops
                    .last()
                    .ok_or_else(|| "native backend found @break outside loop".to_string())?;
                self.module.emit_jmp(&labels.exit);
            }
            GuardAction::Return(expr) => {
                self.emit_expr(expr)?;
                self.emit_return();
                self.returned = true;
            }
            GuardAction::SetAssign { target, value } => {
                self.emit_expr(value)?;
                self.emit_mov_mem_eax(target)?;
                self.module.emit_jmp(next);
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, expr: &Node<Expr>) -> Result<(), String> {
        match &expr.kind {
            Expr::Lit(Literal::Int(value)) => {
                self.emit_bytes(&[0xb8]); // mov eax, imm32
                self.emit_i32(*value as i32);
                Ok(())
            }
            Expr::Lit(Literal::Bool(value)) => {
                self.emit_bytes(&[0xb8]);
                self.emit_i32(if *value { 1 } else { 0 });
                Ok(())
            }
            Expr::Ident(name) => self.emit_mov_eax_mem(name),
            Expr::Paren(inner) => self.emit_expr(inner),
            Expr::Unary { op, operand } => {
                self.emit_expr(operand)?;
                match op {
                    UnOp::Neg => self.emit_bytes(&[0xf7, 0xd8]), // neg eax
                    UnOp::Not => {
                        self.emit_bytes(&[0x83, 0xf8, 0x00]); // cmp eax, 0
                        self.emit_bytes(&[0x0f, 0x94, 0xc0]); // sete al
                        self.emit_bytes(&[0x0f, 0xb6, 0xc0]); // movzx eax, al
                    }
                }
                Ok(())
            }
            Expr::Binary { op, lhs, rhs } => {
                self.emit_expr(lhs)?;
                self.emit_bytes(&[0x50]); // push rax
                self.emit_expr(rhs)?;
                self.emit_bytes(&[0x89, 0xc1]); // mov ecx, eax
                self.emit_bytes(&[0x58]); // pop rax
                self.emit_binary_op(*op);
                Ok(())
            }
            Expr::Call { callee, args } => self.emit_call_expr(callee, args),
            Expr::Cast { expr, .. } => self.emit_expr(expr),
            Expr::Lit(Literal::Float(_)) | Expr::Lit(Literal::Str(_)) | Expr::Ctor(_) => Err(
                "native backend currently lowers executable integer/control-flow code; string, float, Option, and Result values need the runtime ABI"
                    .to_string(),
            ),
        }
    }

    fn emit_call_expr(&mut self, callee: &str, args: &[Node<Expr>]) -> Result<(), String> {
        match callee {
            "debug_i32" => return self.emit_builtin_debug_i32(args),
            "debug_string" => return self.emit_builtin_debug_string(args),
            "abs_i32" => return self.emit_builtin_abs(args),
            "is_even_i32" => return self.emit_builtin_is_even(args),
            "max_i32" => return self.emit_builtin_max(args),
            "vector_len_i32" => return self.emit_builtin_vector_len(args),
            "vector_i32_3" => {
                return Err("native vector_i32_3 must initialize a Vector<i32> binding".to_string());
            }
            _ => {}
        }

        if args.len() > ARG_REGS.len() {
            return Err("native backend currently supports up to 6 integer call arguments".to_string());
        }
        for arg in args {
            self.emit_expr(arg)?;
            self.emit_bytes(&[0x50]); // push rax
        }
        for idx in (0..args.len()).rev() {
            self.emit_bytes(&[0x58]); // pop rax
            self.emit_bytes(&[0x89, ARG_REGS[idx].mov_from_eax_modrm]); // mov arg_reg, eax
        }
        self.module.emit_call(&func_label(callee));
        Ok(())
    }

    fn emit_builtin_debug_i32(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("debug_i32", args, 1)?;
        self.emit_expr(&args[0])?;
        self.emit_bytes(&[0x89, 0xc7]); // mov edi, eax
        self.module.emit_call("runtime.debug_i32");
        self.emit_bytes(&[0x31, 0xc0]); // debug_i32 returns unit-like zero
        Ok(())
    }

    fn emit_builtin_debug_string(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("debug_string", args, 1)?;
        let Expr::Lit(Literal::Str(value)) = &args[0].kind else {
            return Err("native debug_string currently requires a string literal".to_string());
        };
        let label = self
            .module
            .add_static_bytes("string.literal", value.as_bytes().to_vec());
        self.emit_bytes(&[0xb8]);
        self.emit_i32(1); // mov eax, SYS_write
        self.emit_bytes(&[0xbf]);
        self.emit_i32(1); // mov edi, stdout
        self.emit_bytes(&[0x48, 0xbe]); // mov rsi, imm64
        self.module.emit_abs64_patch(&label);
        self.emit_bytes(&[0xba]);
        self.emit_i32(value.len() as i32); // mov edx, len
        self.emit_bytes(&[0x0f, 0x05]); // syscall
        self.emit_bytes(&[0x31, 0xc0]); // debug_string returns unit-like zero
        Ok(())
    }

    fn emit_builtin_abs(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("abs_i32", args, 1)?;
        self.emit_expr(&args[0])?;
        self.emit_bytes(&[0x89, 0xc1]); // mov ecx, eax
        self.emit_bytes(&[0xf7, 0xd9]); // neg ecx
        self.emit_bytes(&[0x83, 0xf8, 0x00]); // cmp eax, 0
        self.emit_bytes(&[0x0f, 0x48, 0xc1]); // cmovs eax, ecx
        Ok(())
    }

    fn emit_builtin_is_even(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("is_even_i32", args, 1)?;
        self.emit_expr(&args[0])?;
        self.emit_bytes(&[0x83, 0xe0, 0x01]); // and eax, 1
        self.emit_bytes(&[0x83, 0xf8, 0x00]); // cmp eax, 0
        self.emit_bytes(&[0x0f, 0x94, 0xc0]); // sete al
        self.emit_bytes(&[0x0f, 0xb6, 0xc0]); // movzx eax, al
        Ok(())
    }

    fn emit_builtin_max(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("max_i32", args, 2)?;
        self.emit_expr(&args[0])?;
        self.emit_bytes(&[0x50]); // push rax
        self.emit_expr(&args[1])?;
        self.emit_bytes(&[0x89, 0xc1]); // mov ecx, eax
        self.emit_bytes(&[0x58]); // pop rax
        self.emit_bytes(&[0x39, 0xc8]); // cmp eax, ecx
        self.emit_bytes(&[0x0f, 0x4c, 0xc1]); // cmovl eax, ecx
        Ok(())
    }

    fn emit_builtin_vector_len(&mut self, args: &[Node<Expr>]) -> Result<(), String> {
        require_arg_count("vector_len_i32", args, 1)?;
        let Expr::Ident(name) = &args[0].kind else {
            return Err("native vector_len_i32 currently requires a vector identifier".to_string());
        };
        self.emit_mov_eax_mem_at(name, 8)
    }

    fn emit_vector_i32_3_let(&mut self, stmt: &LetStmt) -> Result<bool, String> {
        let Expr::Call { callee, args } = &stmt.value.kind else {
            return Ok(false);
        };
        if callee != "vector_i32_3" {
            return Ok(false);
        }
        require_arg_count("vector_i32_3", args, 3)?;
        if !matches!(stmt.binding.ty, Type::Vector(_)) {
            return Err("native vector_i32_3 must initialize a Vector<i32> binding".to_string());
        }

        let data_name = vec_data_name(&stmt.binding.name);
        for (idx, arg) in args.iter().enumerate() {
            self.emit_expr(arg)?;
            self.emit_mov_mem_eax_at(&data_name, (idx as i32) * 4)?;
        }

        self.emit_lea_rax_slot(&data_name, 0)?;
        self.emit_mov_mem_rax_qword(&stmt.binding.name, 0)?;
        self.emit_mov_qword_mem_imm32_at(&stmt.binding.name, 8, 3)?;
        self.emit_mov_qword_mem_imm32_at(&stmt.binding.name, 16, 3)?;
        Ok(true)
    }

    fn emit_binary_op(&mut self, op: BinOp) {
        match op {
            BinOp::Add => self.emit_bytes(&[0x01, 0xc8]), // add eax, ecx
            BinOp::Sub => self.emit_bytes(&[0x29, 0xc8]), // sub eax, ecx
            BinOp::Mul => self.emit_bytes(&[0x0f, 0xaf, 0xc1]), // imul eax, ecx
            BinOp::Div => {
                self.emit_bytes(&[0x99]); // cdq
                self.emit_bytes(&[0xf7, 0xf9]); // idiv ecx
            }
            BinOp::Mod => {
                self.emit_bytes(&[0x99]); // cdq
                self.emit_bytes(&[0xf7, 0xf9]); // idiv ecx
                self.emit_bytes(&[0x89, 0xd0]); // mov eax, edx
            }
            BinOp::And => self.emit_bytes(&[0x21, 0xc8]), // and eax, ecx
            BinOp::Or => self.emit_bytes(&[0x09, 0xc8]),  // or eax, ecx
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                self.emit_bytes(&[0x39, 0xc8]); // cmp eax, ecx
                let cc = match op {
                    BinOp::Eq => 0x94,
                    BinOp::Neq => 0x95,
                    BinOp::Lt => 0x9c,
                    BinOp::Le => 0x9e,
                    BinOp::Gt => 0x9f,
                    BinOp::Ge => 0x9d,
                    _ => unreachable!(),
                };
                self.emit_bytes(&[0x0f, cc, 0xc0]); // setcc al
                self.emit_bytes(&[0x0f, 0xb6, 0xc0]); // movzx eax, al
            }
        }
    }

    fn emit_mov_eax_mem(&mut self, name: &str) -> Result<(), String> {
        self.emit_mov_eax_mem_at(name, 0)
    }

    fn emit_mov_eax_mem_at(&mut self, name: &str, field_offset: i32) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x8b, 0x85]); // mov eax, dword ptr [rbp+disp32]
        self.emit_i32(disp);
        Ok(())
    }

    fn emit_mov_mem_eax(&mut self, name: &str) -> Result<(), String> {
        self.emit_mov_mem_eax_at(name, 0)
    }

    fn emit_mov_mem_eax_at(&mut self, name: &str, field_offset: i32) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x89, 0x85]); // mov dword ptr [rbp+disp32], eax
        self.emit_i32(disp);
        Ok(())
    }

    fn emit_lea_rax_slot(&mut self, name: &str, field_offset: i32) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x48, 0x8d, 0x85]); // lea rax, [rbp+disp32]
        self.emit_i32(disp);
        Ok(())
    }

    fn emit_mov_rax_mem_qword(&mut self, name: &str, field_offset: i32) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x48, 0x8b, 0x85]); // mov rax, qword ptr [rbp+disp32]
        self.emit_i32(disp);
        Ok(())
    }

    fn emit_mov_mem_rax_qword(&mut self, name: &str, field_offset: i32) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x48, 0x89, 0x85]); // mov qword ptr [rbp+disp32], rax
        self.emit_i32(disp);
        Ok(())
    }

    fn emit_mov_qword_mem_imm32_at(
        &mut self,
        name: &str,
        field_offset: i32,
        value: i32,
    ) -> Result<(), String> {
        let disp = self.disp(name, field_offset)?;
        self.emit_bytes(&[0x48, 0xc7, 0x85]); // mov qword ptr [rbp+disp32], imm32
        self.emit_i32(disp);
        self.emit_i32(value);
        Ok(())
    }

    fn disp(&self, name: &str, field_offset: i32) -> Result<i32, String> {
        self.locals
            .get(name)
            .map(|slot| -slot.offset + field_offset)
            .ok_or_else(|| format!("native backend could not resolve local: {name}"))
    }

    fn emit_return(&mut self) {
        self.emit_bytes(&[0x48, 0x89, 0xec]); // mov rsp, rbp
        self.emit_bytes(&[0x5d]); // pop rbp
        self.emit_bytes(&[0xc3]); // ret
    }

    fn emit_bytes(&mut self, bytes: &[u8]) {
        self.module.emit_bytes(bytes);
    }

    fn emit_i32(&mut self, value: i32) {
        self.module.emit_i32(value);
    }
}

fn wrap_elf(code: Vec<u8>) -> Vec<u8> {
    let file_size = CODE_OFFSET + code.len();
    let entry = BASE_ADDR + CODE_OFFSET as u64;
    let mut out = Vec::with_capacity(file_size);

    out.extend_from_slice(b"\x7fELF");
    out.extend_from_slice(&[2, 1, 1, 0]); // ELF64, little-endian, current version
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    out.extend_from_slice(&62u16.to_le_bytes()); // x86_64
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&entry.to_le_bytes());
    out.extend_from_slice(&(ELF_HEADER_SIZE as u64).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(ELF_HEADER_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&(PROGRAM_HEADER_SIZE as u16).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    out.extend_from_slice(&5u32.to_le_bytes()); // PF_R | PF_X
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&BASE_ADDR.to_le_bytes());
    out.extend_from_slice(&BASE_ADDR.to_le_bytes());
    out.extend_from_slice(&(file_size as u64).to_le_bytes());
    out.extend_from_slice(&(file_size as u64).to_le_bytes());
    out.extend_from_slice(&0x1000u64.to_le_bytes());

    out.extend_from_slice(&code);
    out
}

fn func_label(name: &str) -> String {
    format!("func.{}", sanitize(name))
}

fn loop_index_name(node_id: &str) -> String {
    format!("__idx_{}", sanitize(node_id))
}

fn vec_data_name(binding: &str) -> String {
    format!("__vecdata_{}", sanitize(binding))
}

fn is_vector_i32_3_call(expr: &Node<Expr>) -> bool {
    matches!(&expr.kind, Expr::Call { callee, .. } if callee == "vector_i32_3")
}

fn require_arg_count(name: &str, args: &[Node<Expr>], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "native builtin {name} expected {expected} arguments but got {}",
            args.len()
        ))
    }
}

fn type_slot_size(ty: &Type) -> i32 {
    match ty {
        Type::Vector(_) | Type::String => 24,
        Type::Option(_) | Type::Result(_, _) => 16,
        _ => 8,
    }
}

fn is_native_int(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::Bool
    )
}

fn align_to(value: i32, align: i32) -> i32 {
    ((value + align - 1) / align) * align
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' { ch } else { '_' })
        .collect()
}
