use std::ffi::CString;
use std::ptr;
use std::str;
use std::mem;
use libc::c_void;
use gl;
use gl::types::{GLfloat, GLenum, GLuint, GLint, GLchar, GLsizeiptr};
use gl::types::{GLboolean};
use sdl2::video::{Window, WindowPos, GLAttr, OPENGL, GLContext};
use sdl2::video::{gl_set_attribute, gl_get_proc_address};
use sdl2::sdl::Sdl;
use std::iter::repeat;

use gpu::AlphaColor;
use gpu::Color;

/// OpenGL-based rendering
pub struct OpenGL {
    /// SDL2 window
    window:  Window,
    /// OpenGL context
    #[allow(dead_code)]
    context: GLContext,
    /// texture representing the GameBoy framebuffer.
    texture: [u8; 160 * 144 * 4 * 2],
}

impl OpenGL {
    pub fn new(sdl2: &Sdl, xres: u32, yres: u32) -> OpenGL {
        gl_set_attribute(GLAttr::GLContextMajorVersion, 3);
        gl_set_attribute(GLAttr::GLContextMinorVersion, 3);

        gl_set_attribute(GLAttr::GLDoubleBuffer, 1);
        gl_set_attribute(GLAttr::GLDepthSize, 24);
        gl_set_attribute(GLAttr::GLMultiSampleSamples, 4);

        let window = match Window::new(sdl2,
                                       "gb-rs",
                                       WindowPos::PosCentered,
                                       WindowPos::PosCentered,
                                       xres as i32, yres as i32,
                                       OPENGL) {
            Ok(window) => window,
            Err(err)   => panic!("failed to create SDL2 window: {}", err)
        };

        let context = match window.gl_create_context() {
            Ok(context) => context,
            Err(err)    => panic!("failed to create OpenGL context: {}", err),
        };

        // Load OpenGL function pointers from SDL2
        ::gl::load_with(|s| {
            match gl_get_proc_address(s) {
                Some(p) => p as *const c_void,
                None    => panic!("can't get proc address for {}", s),
            }
        });

        let vertex_shader =
            compile_shader(
                "#version 330 core                                \n\
                                                                  \n\
                 in  vec3 position;                               \n\
                 in  vec2 vertex_uv;                              \n\
                                                                  \n\
                 out vec2 uv;                                     \n\
                 const mat4 rot = mat4(                           \n\
                       vec4(0.707, 0.0, 0.707, 0.0),                  \n\
                       vec4(0.0, 1.0, 0.0, 0.0),                  \n\
                       vec4(-0.707, 0.0, 0.707, 0.0),                  \n\
                       vec4(0.0, 0.0, 0.0, 1.0)                   \n\
                    );                                            \n\
                                                                  \n\
                 const mat4 scal = mat4(                          \n\
                       vec4(1.0, 0.0, 0.0, 0.0),                  \n\
                       vec4(0.0, 1.0, 0.0, 0.0),                  \n\
                       vec4(0.0, 0.0, 1.0, 0.0),                  \n\
                       vec4(0.0, 0.0, 0.0, 1.0)                   \n\
                    );                                            \n\
                                                                  \n\
                 const mat4 trans = mat4(                         \n\
                       vec4(1.0, 0.0, 0.0, -1.0),                  \n\
                       vec4(0.0, 1.0, 0.0, 0.0),                  \n\
                       vec4(0.0, 0.0, 1.0, -2.0),                  \n\
                       vec4(0.0, 0.0, 0.0, 1.0)                   \n\
                    );                                            \n\
                                                                  \n\
                 const mat4 view = rot * trans * scal;            \n\
                                                                  \n\
                 void main(void) {                                \n\
                     gl_Position = view * vec4(position, 1.0);    \n\
                     uv = vertex_uv;                              \n\
                 }",
                gl::VERTEX_SHADER);

        let fragment_shader =
            compile_shader(
                "#version 330 core                                \n\
                                                                  \n\
                 in  vec2 uv;                                     \n\
                                                                  \n\
                 out vec4 color;                                  \n\
                                                                  \n\
                 uniform sampler2D gb_screen;                     \n\
                                                                  \n\
                 void main(void) {                                \n\
                     color = texture2D(gb_screen, uv);            \n\
                 }",
                gl::FRAGMENT_SHADER);

        let program = link_program(vertex_shader, fragment_shader);

        let bg_top_left  = (-0.8,  0.8);
        let bg_top_right = ( 1.0,  0.8);
        let bg_bot_left  = (-0.8, -0.8);
        let bg_bot_right = ( 1.0, -0.8);

        let sp_top_left  = (-1.0,  0.8);
        let sp_top_right = ( 0.5,  0.8);
        let sp_bot_left  = (-1.0, -0.8);
        let sp_bot_right = ( 0.5, -0.8);


        let vertices: [GLfloat; 36] = [
            bg_top_left.0, bg_top_left.1,   -0.9,
            bg_top_right.0, bg_top_right.1, -0.9,
            bg_bot_right.0, bg_bot_right.1, -0.9,

            bg_top_left.0, bg_top_left.1,   -0.9,
            bg_bot_right.0, bg_bot_right.1, -0.9,
            bg_bot_left.0, bg_bot_left.1,   -0.9,

            sp_top_left.0, sp_top_left.1,   -0.3,
            sp_top_right.0, sp_top_right.1, -0.3,
            sp_bot_right.0, sp_bot_right.1, -0.3,

            sp_top_left.0, sp_top_left.1,   -0.3, 
            sp_bot_right.0, sp_bot_right.1, -0.3,
            sp_bot_left.0, sp_bot_left.1,   -0.3,
            ];

        // We crop the texture to the actual screen resolution
        let u_max = 159. / 255.;
        let v_bg_min = 0.;
        let v_bg_max = (143. / 255.) / 2.;
        let v_sp_min = (144. / 255.) / 2.;
        let v_sp_max = 143. / 255.;

        let uv_mapping: [GLfloat; 24] = [
            0.,    v_bg_min,
            u_max, v_bg_min,
            u_max, v_bg_max,

            0.,    v_bg_min,
            u_max, v_bg_max,
            0.,    v_bg_max,

            0.,    v_sp_min,
            u_max, v_sp_min,
            u_max, v_sp_max,

            0.,    v_sp_min,
            u_max, v_sp_max,
            0.,    v_sp_max,
            ];

        let mut vertex_array_object  = 0;
        let mut vertex_buffer_object = 0;
        let mut uv_buffer_object = 0;
        let mut texture = 0;
        let mut texture_id;

        unsafe {
            gl::GenVertexArrays(1, &mut vertex_array_object);
            gl::BindVertexArray(vertex_array_object);

            // Generate vertex buffer
            gl::GenBuffers(1, &mut vertex_buffer_object);
            gl::BindBuffer(gl::ARRAY_BUFFER, vertex_buffer_object);

            let pos_attr = gl::GetAttribLocation(program,
                                                 CString::new("position").unwrap().as_ptr());

            gl::EnableVertexAttribArray(pos_attr as GLuint);
            gl::VertexAttribPointer(pos_attr as GLuint, 3, gl::FLOAT,
                                    gl::FALSE as GLboolean, 0, ptr::null());

            gl::BufferData(gl::ARRAY_BUFFER,
                           (vertices.len() * mem::size_of::<GLfloat>()) as GLsizeiptr,
                           mem::transmute(&vertices[0]),
                           gl::STATIC_DRAW);

            // Generate uv buffer
            gl::GenBuffers(1, &mut uv_buffer_object);
            gl::BindBuffer(gl::ARRAY_BUFFER, uv_buffer_object);

            let pos_attr = gl::GetAttribLocation(program,
                                                 CString::new("vertex_uv").unwrap().as_ptr());

            gl::EnableVertexAttribArray(pos_attr as GLuint);
            gl::VertexAttribPointer(pos_attr as GLuint, 2, gl::FLOAT,
                                    gl::FALSE as GLboolean, 0, ptr::null());


            gl::BufferData(gl::ARRAY_BUFFER,
                           (uv_mapping.len() * mem::size_of::<GLfloat>()) as GLsizeiptr,
                           mem::transmute(&uv_mapping[0]),
                           gl::STATIC_DRAW);

            // Create the texture used to render the GB screen
            gl::GenTextures(1, &mut texture);
            gl::BindTexture(gl::TEXTURE_2D, texture);

            gl::TexParameteri(gl::TEXTURE_2D,
                              gl::TEXTURE_MAG_FILTER,
                              gl::NEAREST as GLint);

            gl::TexParameteri(gl::TEXTURE_2D,
                              gl::TEXTURE_MIN_FILTER,
                              gl::NEAREST as GLint);

            gl::TexStorage2D(gl::TEXTURE_2D,
                             // Only one layer
                             1,
                             gl::RGBA8,
                             // I use a 256x256 textures because
                             // apparently power-of-two textures are
                             // potentially faster in openGL.
                             256, 512);

            texture_id = gl::GetUniformLocation(program,
                                                CString::new("gb_screen").unwrap().as_ptr());

            gl::Uniform1i(texture_id, texture as GLint);

            // Use shader program
            gl::UseProgram(program);

            gl::BindFragDataLocation(program, 0,
                                     CString::new("color").unwrap().as_ptr());

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::ClearColor(0.83, 0.84, 0.94, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }

        OpenGL {
            window:  window,
            context: context,
            texture: [0; 160 * 144 * 4 * 2],
        }
    }
}

impl ::ui::Display for OpenGL {

    fn clear(&mut self) {
        for b in self.texture.iter_mut() {
            *b = 0;
        }
    }

    fn set_bg_pixel(&mut self, x: u32, y: u32, color: AlphaColor) {
        let alpha = match color.opaque {
            true => 0xff,
            false => 0xff,
        };

        let color = match color.color {
            Color::Black     => [0x00, 0x00, 0x00],
            Color::DarkGrey  => [0x55 / 2, 0x55 / 2, 0x55 / 2],
            Color::LightGrey => [0xab / 2, 0xab / 2, 0xab / 2],
            Color::White     => [0xff / 2, 0xff / 2, 0xff / 2],
        };

        let pos = y * (160 * 4) + x * 4;
        let pos = pos as usize;

        self.texture[pos + 0] = color[0];
        self.texture[pos + 1] = color[1];
        self.texture[pos + 2] = color[2];
        self.texture[pos + 3] = alpha;
    }

    fn set_sprite_pixel(&mut self, x: u32, y: u32, color: AlphaColor) {
        let (alpha, color) = match color.opaque {
            true => {
                let color = match color.color {
                    Color::Black     => [0x00, 0x00, 0x00],
                    Color::DarkGrey  => [0x55, 0x55, 0x55],
                    Color::LightGrey => [0xab, 0xab, 0xab],
                    Color::White     => [0xff, 0xff, 0xff],
                };

                (0xff, color)
            }
            false => (0x3f, [0x20, 0x00, 0x7f]),
        };

        

        let pos = y * (160 * 4) + x * 4 + (160 * 144 * 4);
        let pos = pos as usize;

        self.texture[pos + 0] = color[0];
        self.texture[pos + 1] = color[1];
        self.texture[pos + 2] = color[2];
        self.texture[pos + 3] = alpha;
    }

    fn flip(&mut self) {
        unsafe {
            gl::TexSubImage2D(gl::TEXTURE_2D,
                              0,
                              // Offset in the texture
                              0, 0,
                              // Dimensions of the updated part
                              160, 144 * 2,
                              gl::RGBA,
                              gl::UNSIGNED_BYTE,
                              mem::transmute(&self.texture[0]));

            gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::DrawArrays(gl::TRIANGLES, 0, 12);
        }

        self.window.gl_swap_window();
        self.clear();
    }
}

fn compile_shader(src: &str, ty: GLenum) -> GLuint {
    let shader;
    unsafe {
        shader = gl::CreateShader(ty);
        // Attempt to compile the shader
        let c_str = CString::new(src).unwrap();
        gl::ShaderSource(shader, 1, &c_str.as_ptr(), ptr::null());
        gl::CompileShader(shader);

        // Get the compile status
        let mut status = gl::FALSE as GLint;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
            // subtract 1 to skip the trailing null character
            let mut buf: Vec<_> = repeat(0).take(len as usize - 1).collect();
            gl::GetShaderInfoLog(shader,
                                 len, ptr::null_mut(),
                                 buf.as_mut_ptr() as *mut GLchar);
            panic!("{}",
                   str::from_utf8(&buf).ok()
                   .expect("ShaderInfoLog not valid utf8"));
        }
    }
    shader
}

fn link_program(vs: GLuint, fs: GLuint) -> GLuint {
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vs);
        gl::AttachShader(program, fs);
        gl::LinkProgram(program);
        // Get the link status
        let mut status = gl::FALSE as GLint;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len: GLint = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf: Vec<_> = repeat(0).take(len as usize - 1).collect();
            gl::GetProgramInfoLog(program,
                                  len,
                                  ptr::null_mut(),
                                  buf.as_mut_ptr() as *mut GLchar);
            panic!("{}",
                   str::from_utf8(&buf).ok()
                   .expect("ProgramInfoLog not valid utf8"));
        }
        program
    }
}
