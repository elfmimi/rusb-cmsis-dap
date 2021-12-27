#![allow(unused)]
#![allow(unused_imports)]
#![allow(non_upper_case_globals)]
use rusb::{Context, DeviceHandle, Error, UsbContext, Device, request_type, Direction, RequestType, Recipient};
use rusb::{Language};

use std::fmt;
use std::time::Duration;
use std::convert::TryInto;

const USB_VID: u16 = 0x0D28;
const USB_PID: u16 = 0x0204;

fn is_cmsis_dap_device<T: UsbContext>(device: &Device<T>) -> bool {
    // Check the VID/PID.
    if let Ok(descriptor) = device.device_descriptor() {
        (descriptor.vendor_id() == USB_VID)
            && (descriptor.product_id() == USB_PID)
    } else {
        false
    }
}

fn main() {
    // pretty_env_logger::init();
    {
        log::set_max_level(log::LevelFilter::Off);
        let mut builder = pretty_env_logger::formatted_builder();

        let environment_variable_name = "RUST_LOG";
        if let Ok(s) = ::std::env::var(environment_variable_name) {
            builder.parse_filters(&s);
        } else {
/*
            match matches.occurrences_of("v") {
                0 => builder.parse_filters("warn"),
                1 => builder.parse_filters("info"),
                2 => builder.parse_filters("debug"),
                3 | _ => builder.parse_filters("trace"),
            };
*/
            builder.parse_filters("info");
        }

        builder.try_init().unwrap();
    }

    log::trace!("initialized logger");

    match rusb_test() {
        Ok(_) => println!("OK"),
        e => println!("ERROR {:?}", e),
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ProbeCreationError {
    #[error("Probe was not found.")]
    NotFound,
    #[error("USB device could not be opened. Please check the permissions.")]
    CouldNotOpen,
    // #[error("{0}")]
    // HidApi(#[from] hidapi::HidError),
    #[error("{0}")]
    Rusb(#[from] rusb::Error),
    #[error("An error specific to a probe type occured: {0}")]
    ProbeSpecific(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    Other(&'static str),
}

fn dump_buf(buf: &[u8]) {
    let len = buf.len();
    // println!("len = {}", len);
    for i in 0..len {
        print!("{:02X}", buf[i]);
        if i % 16 == 15 || i == len - 1 {
            println!();
        } else if i % 16 == 7 {
            print!(",   ");
        } else {
            print!(", ");
        }
    }
}


trait DeviceHandleEx {
    fn read(&mut self, if_num: u8, in_ep: u8, buf: &mut [u8]) -> Result<usize, ProbeCreationError>;
    fn write(&mut self, if_num: u8, out_ep: u8, buf: &[u8]) -> Result<usize, ProbeCreationError>;
}

impl<T: UsbContext> DeviceHandleEx for DeviceHandle<T> {
    fn read(&mut self, if_num: u8, in_ep: u8, buf: &mut [u8]) -> Result<usize, ProbeCreationError> {
        let timeout = Duration::from_secs(5);
        let len =
            if in_ep == 0 {
                // GET_REPORT
                self.read_control(0xA1, 0x01, 0x0100, if_num as u16, buf, timeout)?
            } else {
                // maybe it's better to use read_interrput() for HID interface. but it works.
                self.read_bulk(in_ep, buf, timeout)?
            };
        Ok(len)
    }
    fn write(&mut self, if_num: u8, out_ep: u8, buf: &[u8]) -> Result<usize, ProbeCreationError> {
        let timeout = Duration::from_secs(5);
        let len =
            if  out_ep == 0 {
                // SET_REPORT
                self.write_control(0x21, 0x09, 0x0200, if_num as u16, &buf, timeout)?
            } else {
                // maybe it's better to use write_interrput() for HID interface. but it works.
                self.write_bulk(out_ep, &buf, timeout)?
            };
        Ok(len)
    }
}

fn rusb_test() -> Result<(), ProbeCreationError> {

    let context = Context::new()?;
    
    let device = context
        .devices()?
        .iter()
        .filter(is_cmsis_dap_device)
        .find_map(|device| {
            Some(device)
        })
        .map_or(Err(ProbeCreationError::NotFound), Ok)?;
    
    let mut device_handle = device.open()?;

    log::debug!("Aquired handle for probe");

    let config = device.active_config_descriptor()?;

    log::debug!("Active config descriptor: {:?}", &config);    


    let descriptor = device.device_descriptor()?;

    log::debug!("Device descriptor: {:?}", &descriptor);

    {
        for interface in config.interfaces() {
            for interface_desc in interface.descriptors() {
                log::debug!("Interface Desc: {:?}", interface_desc);
                let languages = device_handle.read_languages(Duration::from_secs(2)).unwrap();
                for lang in languages {
                    log::debug!("Lang: {:?} {:#06X}", lang, lang.lang_id());
                    let if_str = device_handle.read_interface_string(lang, &interface_desc, Duration::from_secs(2)).unwrap();
                    log::debug!("Interface String: {}", if_str);
                }
                if let Some(n) = interface_desc.description_string_index() {
                    log::debug!("string[{}] = {}", n, device_handle.read_string_descriptor_ascii(n).unwrap())
                }
            }
        }
    }

    // device_handle.unconfigure();
    // device_handle.set_active_configuration(1);
    let languages = device_handle.read_languages(Duration::from_secs(2)).unwrap();
    for lang in languages {
        log::debug!("Lang: {:?} {:#06X}", lang, lang.lang_id());
    }

    log::debug!("string[1] = {}", device_handle.read_string_descriptor_ascii(1).unwrap());
    log::debug!("string[2] = {}", device_handle.read_string_descriptor_ascii(2).unwrap());
    log::debug!("string[3] = {}", device_handle.read_string_descriptor_ascii(3).unwrap());

    // device_handle.set_alternate_setting(0, 0)?;

    // log::debug!("Done set interface alternate setting of interface 0.");

    // let out_ep = 0x01;
    // let in_ep = 0x81;

    let use_hid_out_ep = false;
    let use_cmsis_dap_v2 = true;
    let (if_num, out_ep, in_ep) =
    {
        let mut if_num = 0;
        let mut out_ep = 0;
        let mut in_ep = 0;
        // Search CMSIS-DAP v1 interface
        for interface in config.interfaces() {
            if let Some(descriptor) = interface.descriptors().next() {
                if let Some(string_index) = descriptor.description_string_index() {
                    let interface_string = device_handle.read_string_descriptor_ascii(string_index).unwrap();
                    println!("interface {} : {}", interface.number(), interface_string);
                    // if interface_string.starts_with("CMSIS-DAP v1") || interface_string.starts_with("CMSIS-DAP-v1"){
                    let cc_sub_prot = (descriptor.class_code(), descriptor.sub_class_code(), descriptor.protocol_code());
                    if cc_sub_prot == (0x03, 0x00, 0x00) {
                        if_num = interface.number();
                        for endpoint in descriptor.endpoint_descriptors() {
                            println!("interface {} ep {:#04X}", interface.number(), endpoint.address());
                            let ep = endpoint.address();
                            if ep & 0x80 != 0 {
                                in_ep = ep;
                            } else if use_hid_out_ep {
                                out_ep = ep;
                            }
                        }
                    }
                }
            }
        }
        // Search CMISIS-DAP v2 interface and override with it
        for interface in config.interfaces() {
            if let Some(descriptor) = interface.descriptors().next() {
                if let Some(string_index) = descriptor.description_string_index() {
                    let interface_string = device_handle.read_string_descriptor_ascii(string_index).unwrap();
                    // println!("interface {} : {}", interface.number(), interface_string);
                    if interface_string.starts_with("CMSIS-DAP v2") && use_cmsis_dap_v2 {
                        if_num = interface.number();
                        for endpoint in descriptor.endpoint_descriptors() {
                            println!("interface {} ep {:#04X}", interface.number(), endpoint.address());
                            let ep = endpoint.address();
                            if ep & 0x80 != 0 {
                                in_ep = ep;
                            } else {
                                out_ep = ep;
                            }
                        }
                        if in_ep == 0 || out_ep == 0 {
                            in_ep = 0;
                            out_ep = 0;
                            if_num = 0;
                        }
                    }
                }
            }
        }
        (if_num, out_ep, in_ep)
    };
    log::debug!("if_num = {}", if_num);
    log::debug!("out_ep = {:#04X}", out_ep);
    log::debug!("in_ep = {:#04X}", in_ep);
    device_handle.claim_interface(if_num)?;
    log::debug!("Claimed interface {} of USB device.", if_num);

    // device_handle.clear_halt(0x01);
    // device_handle.clear_halt(0x81);

    const ID_DAP_Info: u8 = 0x00;
    const ID_DAP_Connect: u8 = 0x02;
    const ID_DAP_Transfer: u8 = 0x05;
    const ID_DAP_SWJ_Clock: u8 = 0x11;
    const ID_DAP_SWJ_Sequence: u8 = 0x12;
    const ID_DAP_QueueCommands: u8 = 0x7E;
    const ID_DAP_ExecuteCommands: u8 = 0x7F;

    // ID_DAP_Info
    const DAP_ID_VENDOR: u8 = 0x01;
    const DAP_ID_PRODUCT: u8 = 0x02;
    const DAP_ID_SER_NUM: u8 = 0x03;
    const DAP_ID_FW_VER: u8 = 0x04;
    const DAP_ID_DEVICE_VENDOR: u8 = 0x05;
    const DAP_ID_DEVICE_NAME: u8 = 0x06;
    const DAP_ID_BOARD_VENDOR: u8 = 0x07;
    const DAP_ID_BOARD_NAME: u8 = 0x08;
    const DAP_ID_PRODUCT_FW_VER: u8 = 0x09;

    // ID_DAP_Connect
    const DAP_PORT_SWD: u8 = 0x01;
    const DAP_PORT_JTAG: u8 = 0x02;

    let mut cmds = Vec::new();
    let mut checkers: Vec<Box<dyn Fn(&[u8]) -> usize>> = Vec::new();
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_VENDOR);
    add_info_str(&mut cmds, &mut checkers, DAP_ID_PRODUCT);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_SER_NUM);
    add_info_str(&mut cmds, &mut checkers, DAP_ID_FW_VER);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_DEVICE_VENDOR);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_DEVICE_NAME);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_BOARD_VENDOR);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_BOARD_NAME);
    add_info_str(&mut cmds, &mut checkers, DAP_ID_PRODUCT_FW_VER);

    let mut buf = [0u8; 64];
    buf[0] = ID_DAP_ExecuteCommands;
    buf[1] = checkers.len() as u8;
    assert!(cmds.len() <= 64 - 2);
    buf[2..(2+cmds.len())].copy_from_slice(cmds.as_ref());
    let len = device_handle.write(if_num, out_ep, &buf)?;
    println!("write len = {}", len);

/***/

    let mut buf = [0u8; 64];
    buf[0] = 0;
    buf[1] = 0;
    let len = device_handle.read(if_num, in_ep, &mut buf)?;
    println!("read len = {}", len);
    dump_buf(&buf[..len]);
    assert!(buf[0] == ID_DAP_ExecuteCommands);
    assert!(buf[1] == checkers.len() as u8);
    let mut ptr = 2;
    for checker in &checkers {
        ptr = ptr + checker(&buf[ptr..]);
    }
    assert!(ptr <= len);

/***/

    let mut cmds = Vec::new();
    let mut checkers: Vec<Box<dyn Fn(&[u8]) -> usize>> = Vec::new();
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_VENDOR);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_PRODUCT);
    add_info_str(&mut cmds, &mut checkers, DAP_ID_SER_NUM);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_FW_VER);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_DEVICE_VENDOR);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_DEVICE_NAME);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_BOARD_VENDOR);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_BOARD_NAME);
    // add_info_str(&mut cmds, &mut checkers, DAP_ID_PRODUCT_FW_VER);
    add_connect(&mut cmds, &mut checkers);
    // add_set_clock(&mut cmds, &mut checkers, 0x00000100); // 256Hz
    // add_set_clock(&mut cmds, &mut checkers, 0x00100000); // 1MHz
    add_set_clock(&mut cmds, &mut checkers, 0x01000000); // 16MHz
    add_jtag_to_swd_sequence(&mut cmds, &mut checkers);
    add_swd_reset_sequence(&mut cmds, &mut checkers);

    let mut buf = [0u8; 64];
    buf[0] = ID_DAP_ExecuteCommands;
    buf[1] = checkers.len() as u8;
    assert!(cmds.len() <= 64 - 2);
    buf[2..(2+cmds.len())].copy_from_slice(cmds.as_ref());
    let len = device_handle.write(if_num, out_ep, &buf)?;
    println!("write len = {}", len);

/***/

    let mut buf = [0u8; 64];
    buf[0] = 0;
    buf[1] = 0;
    let len = device_handle.read(if_num, in_ep, &mut buf)?;
    println!("read len = {}", len);
    dump_buf(&buf[..len]);
    assert!(buf[0] == ID_DAP_ExecuteCommands);
    assert!(buf[1] == checkers.len() as u8);
    let mut ptr = 2;
    for checker in &checkers {
        ptr = ptr + checker(&buf[ptr..]);
    }
    assert!(ptr <= len);

/***/

    let mut cmds = Vec::new();
    let mut checkers: Vec<Box<dyn Fn(&[u8]) -> usize>> = Vec::new();
    add_init_transfer(&mut cmds, &mut checkers);

    let mut buf = [0u8; 64];
    buf[0] = ID_DAP_ExecuteCommands;
    buf[1] = checkers.len() as u8;
    assert!(cmds.len() <= buf.len() - 2);
    buf[2..(2+cmds.len())].copy_from_slice(&cmds.as_ref());
    let len = device_handle.write(if_num, out_ep, &buf)?;
    log::debug!("cmds.len() = {}", cmds.len());
    println!("write len = {}", len);

/***/

    let mut buf = [0u8; 64];
    buf[0] = 0;
    buf[1] = 0;
    let len = device_handle.read(if_num, in_ep, &mut buf)?;
    println!("read len = {}", len);
    dump_buf(&buf[..len]);
    assert!(buf[0] == ID_DAP_ExecuteCommands);
    assert!(buf[1] == checkers.len() as u8);
    let mut ptr = 2;
    for checker in &checkers {
        ptr = ptr + checker(&buf[ptr..]);
    }
    assert!(ptr <= len);

/***/

    fn add_info_str(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>, info: u8) {
        cmds.extend([ID_DAP_Info, info]); // ID_DAP_Info, DAP_ID_*
        checkers.push( Box::new(move |buf: &[u8]| -> usize {
            assert!(buf[0] == ID_DAP_Info);
            assert!(buf[(2+buf[1]-1) as usize] == 0); // always has a terminating NUL character
            let ver_name = 
                match info {
                    DAP_ID_VENDOR => String::from("VENDOR"),
                    DAP_ID_PRODUCT => String::from("PRODUCT"),
                    DAP_ID_SER_NUM => String::from("SERIAL_NUMBER"),
                    DAP_ID_FW_VER => String::from("FW_VER"),
                    DAP_ID_PRODUCT_FW_VER => String::from("DAPLINK_VER"),
                    _ => format!("Info {:#04X}", info),
                };
            println!("{} = {}", ver_name, std::str::from_utf8(&buf[2..(2+buf[1]) as usize]).unwrap());
            2 + buf[1] as usize
        }));
    }

    fn add_connect(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>) {
        cmds.extend([ID_DAP_Connect, DAP_PORT_SWD]);
        checkers.push(Box::new(|buf: &[u8]| -> usize {
            assert!(buf[0] == ID_DAP_Connect);
            assert!(buf[1] == DAP_PORT_SWD);
            2
        }));
    }

    fn add_set_clock(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>, clock: u32) {
        cmds.push(ID_DAP_SWJ_Clock);
        // cmds.extend([ID_DAP_SWJ_Clock, 0, 1, 0, 0]); // 256Hz
        // cmds.extend([ID_DAP_SWJ_Clock, 0, 0, 0, 1]); // 16MHz
        cmds.extend(clock.to_le_bytes());
        checkers.push(Box::new(|buf: &[u8]| -> usize {
            assert!(buf[0] == ID_DAP_SWJ_Clock);
            assert!(buf[1] == 0);
            2
        }));
    }

    fn add_swd_reset_sequence(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>) {
        cmds.extend([ID_DAP_SWJ_Sequence,
                     56, // bits
                     0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F
        ]);
        checkers.push(Box::new(|buf: &[u8]| -> usize {
            assert!(buf[0] == ID_DAP_SWJ_Sequence);
            assert!(buf[1] == 0);
            2
        }));
    }

    fn add_jtag_to_swd_sequence(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>) {
        cmds.extend([ID_DAP_SWJ_Sequence,
                     72, // bits
                     0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                     0x9E, 0xE7
        ]);
        checkers.push(Box::new(|buf: &[u8]| -> usize {
            assert!(buf[0] == ID_DAP_SWJ_Sequence);
            assert!(buf[1] == 0);
            2
        }));
    }

    fn add_init_transfer(cmds: &mut Vec<u8>, checkers: &mut Vec<Box<dyn Fn(&[u8]) -> usize>>) {
        let mut transfers = Vec::new();
        let mut readers: Vec<Box<dyn Fn(&[u8]) -> usize>> = Vec::new();
        transfers.push(0x02); // DP_IDCODE | DAP_TRANSFER_RnW
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("IDCODE = {:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));
/*
        transfers.push(0x02); // DP_IDCODE | DAP_TRANSFER_RnW
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("{:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));
*/
        transfers.push(0x00); // DP_ABORT, nW, clear sticky error bits
        transfers.extend(0x0000001Du32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x08); // DP_SELECT, nW, set AP0, AP bank 0xF, DP bank 0
        transfers.extend(0x000000F0u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));

        // Startup Debug Circuit
        transfers.push(0x04); // DP_CTRL/STAT(bank 0), nW, CSYSPWRUPREQ, CDBGPWRUPREQ
        transfers.extend(0x50000000u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x20); // DP, nW, MATCH_MASK
        transfers.extend(0xA0000000u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x16); // DP_CTRL/STAT(bank 0), R, MATCH_VALUE
        transfers.extend(0xA0000000u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));

        // 0x04770021 indicates AHB-AP
        transfers.push(0x0F); // AP_xC(bank 0xF) R (AP_IDR)
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("AP_IDR = {:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));

        transfers.push(0x08); // DP_SELECT, nW, set AP0, AP bank 0x0, DP bank 0
        transfers.extend(0x00000000u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
/*
        transfers.push(0x03); // AP_x0 R (AP_CSW)
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("AP CSW {:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));
*/

        // Read CPUID
        transfers.push(0x01); // AP_x0(bank 0) nW (AP_CSW)
        transfers.extend(0x03000042u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x05); // AP_x4(bank 0) nW (AP_TAR)
        transfers.extend(0xE000ED00u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x0F); // AP_xC(bank 0) R (AP_DRW)
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("0xE000ED00 (CPUID) {:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));

/*
        // Read PDID
/*
        transfers.push(0x01); // AP_x0(bank 0) nW (AP_CSW)
        transfers.extend(0x03000042u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
*/
        transfers.push(0x05); // AP_x4(bank 0) nW (AP_TAR)
        transfers.extend(0x50000000u32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
        transfers.push(0x0F); // AP_xC(bank 0) R (AP_DRW)
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("0x50000000 (PDID) {:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));
*/

/*
        transfers.push(0x02); // DP_IDCODE, R
        readers.push(Box::new(|buf: &[u8]| -> usize {
            println!("{:#010X}", u32::from_le_bytes(buf[0..4].try_into().unwrap()));
            4
        }));
        transfers.push(0x00); // DP_ABORT, nW, clear sticky error bits
        transfers.extend(0x0000001Du32.to_le_bytes());
        readers.push(Box::new(|buf: &[u8]| -> usize { 0 }));
*/
        cmds.extend([ID_DAP_Transfer, 0, readers.len() as u8]);
        cmds.extend(transfers);
        checkers.push(Box::new(move |buf: &[u8]| -> usize {
            let count = readers.len();
            assert!(buf[0] == ID_DAP_Transfer);
            if buf[1] != count as u8{
                println!("Not all transfers completed.");
            }
            if buf[2] != 1 /* OK */ {
                println!("Wrong response code");
            }
            let mut ptr = 3;
            for reader in &readers {
                ptr = ptr + reader(&buf[ptr..]);
            }
            ptr
        }));
    }

/***/

    Ok(())
}
