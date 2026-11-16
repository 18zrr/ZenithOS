//! Process and thread management for Zenith OS kernel

/// Process state enumeration  
#[derive(Debug, Clone, Copy)]
pub enum ProcessState {
    Running,
    Waiting,
    Terminated,
}

/// Basic process structure 
#[derive(Debug, Clone, Copy)]
pub struct Process {
    pub id: u32,
    pub name: &'static str,
    pub state: ProcessState,
    pub stack_pointer: usize,
    pub program_counter: usize,
}

impl Process {
    /// Create a new process
    pub fn new(id: u32, name: &'static str) -> Self {
        Process {
            id,
            name,
            state: ProcessState::Running,
            stack_pointer: 0,
            program_counter: 0,
        }
    }
}

/// Simple process manager 
pub struct ProcessManager {
    processes: [Option<Process>; 32], // Max 32 processes
    next_id: u32,
}

impl ProcessManager {
    /// Create a new process manager  
    pub fn new() -> Self {
        ProcessManager {
            processes: [None; 32],
            next_id: 1,
        }
    }

    /// Create a new process
    pub fn create_process(&mut self, name: &'static str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        
        // Find an empty slot 
        for i in 0..32 {
            if self.processes[i].is_none() {
                self.processes[i] = Some(Process::new(id, name));
                return id;
            }
        }
        
        // No free slots
        0
    }

    /// Get a process by ID  
    pub fn get_process(&self, id: u32) -> Option<&Process> {
        for process in &self.processes {
            if let Some(p) = process {
                if p.id == id {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Terminate a process by ID
    pub fn terminate_process(&mut self, id: u32) -> bool {
        for i in 0..32 {
            if let Some(p) = &self.processes[i] {
                if p.id == id {
                    self.processes[i] = None;
                    return true;
                }
            }
        }
        false
    }
}

/// Global process manager instance
static mut PROCESS_MANAGER: ProcessManager = ProcessManager { processes: [None; 32], next_id: 1 };

/// Initialize the process management system 
pub fn init_process_manager() {
    unsafe {
        PROCESS_MANAGER = ProcessManager::new();
    }
    
    crate::diagnostics::println("Process manager initialized");
}

/// Create a new process
#[allow(static_mut_refs)]
pub fn create_new_process(name: &'static str) -> u32 {
    unsafe {
        PROCESS_MANAGER.create_process(name)
    }
}