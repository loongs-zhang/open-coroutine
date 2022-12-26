use base_coroutine::scheduler::Scheduler;
use once_cell::sync::Lazy;

//sleep相关
#[no_mangle]
pub extern "C" fn sleep(secs: libc::c_uint) -> libc::c_uint {
    let rqtp = libc::timespec {
        tv_sec: secs as i64,
        tv_nsec: 0,
    };
    let mut rmtp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    nanosleep(&rqtp, &mut rmtp);
    rmtp.tv_sec as u32
}

#[no_mangle]
pub extern "C" fn usleep(secs: libc::c_uint) -> libc::c_int {
    let secs = secs as i64;
    let sec = secs / 1_000_000;
    let nsec = (secs % 1_000_000) * 1000;
    let rqtp = libc::timespec {
        tv_sec: sec,
        tv_nsec: nsec,
    };
    let mut rmtp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    nanosleep(&rqtp, &mut rmtp)
}

#[no_mangle]
static NANOSLEEP: Lazy<extern "C" fn(*const libc::timespec, *mut libc::timespec) -> libc::c_int> =
    Lazy::new(|| unsafe {
        std::mem::transmute::<
            _,
            extern "C" fn(*const libc::timespec, *mut libc::timespec) -> libc::c_int,
        >(libc::dlsym(libc::RTLD_NEXT, "nanosleep".as_ptr() as _))
    });

#[no_mangle]
pub extern "C" fn nanosleep(rqtp: *const libc::timespec, rmtp: *mut libc::timespec) -> libc::c_int {
    let mut rqtp = unsafe { *rqtp };
    if rqtp.tv_sec < 0 || rqtp.tv_nsec < 0 {
        return -1;
    }
    let nanos_time = match (rqtp.tv_sec as u64).checked_mul(1_000_000_000) {
        Some(v) => v.checked_add(rqtp.tv_nsec as u64).unwrap_or(u64::MAX),
        None => u64::MAX,
    };
    let timeout_time = timer_utils::add_timeout_time(nanos_time);
    loop {
        let _ = Scheduler::current().try_timeout_schedule(timeout_time);
        // 可能schedule完还剩一些时间，此时本地队列没有任务可做
        let schedule_finished_time = timer_utils::now();
        let left_time = match timeout_time.checked_sub(schedule_finished_time) {
            Some(v) => v,
            None => {
                if !rmtp.is_null() {
                    unsafe {
                        (*rmtp).tv_sec = 0;
                        (*rmtp).tv_nsec = 0;
                    }
                }
                return 0;
            }
        } as i64;
        let sec = left_time / 1_000_000_000;
        let nsec = left_time % 1_000_000_000;
        rqtp = libc::timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        };
        //注意这里获取的是原始系统函数nanosleep的指针
        //相当于libc::nanosleep(&rqtp, rmtp)
        if (Lazy::force(&NANOSLEEP))(&rqtp, rmtp) == 0 {
            return 0;
        }
    }
}