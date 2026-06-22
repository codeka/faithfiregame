pub mod camera;
pub mod renderer;
pub mod scene;

pub fn enumerate_adapters() {
    let instance = wgpu::Instance::default();
    println!("--- Enumerating Available 3D Graphics Adapters ---");
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    for (i, adapter) in adapters.into_iter().enumerate() {
        let info = adapter.get_info();
        println!("Adapter {}:", i);
        println!("  Name:        {}", info.name);
        println!("  Backend:     {:?}", info.backend);
        println!("  Device Type: {:?}", info.device_type);
        println!("  Driver:      {}", info.driver);
    }
    println!("--------------------------------------------------");
}
