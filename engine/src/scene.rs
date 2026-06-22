use glam::Mat4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeData {
    None,
    Triangle,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub local_transform: Mat4,
    pub world_transform: Mat4,
    pub children: Vec<Node>,
    pub data: NodeData,
}

impl Node {
    pub fn new(name: &str, data: NodeData) -> Self {
        Self {
            name: name.to_string(),
            local_transform: Mat4::IDENTITY,
            world_transform: Mat4::IDENTITY,
            children: Vec::new(),
            data,
        }
    }

    pub fn add_child(&mut self, child: Node) {
        self.children.push(child);
    }

    pub fn update_transforms(&mut self, parent_transform: &Mat4) {
        self.world_transform = *parent_transform * self.local_transform;
        for child in &mut self.children {
            child.update_transforms(&self.world_transform);
        }
    }
}
