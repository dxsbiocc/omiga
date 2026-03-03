// Omiga Console - Frontend Application

let API_BASE = '/api';
let API_TOKEN = '';

// Load settings from localStorage
function loadSettings() {
    const host = localStorage.getItem('omiga_api_host');
    const token = localStorage.getItem('omiga_api_token');
    if (host) API_BASE = host.replace(/\/$/, '') + '/api';
    if (token) API_TOKEN = token;
}

// API helper functions
async function apiRequest(endpoint, options = {}) {
    const headers = {
        'Content-Type': 'application/json',
        ...options.headers,
    };

    if (API_TOKEN) {
        headers['Authorization'] = `Bearer ${API_TOKEN}`;
    }

    const response = await fetch(API_BASE + endpoint, {
        ...options,
        headers,
    });

    if (!response.ok) {
        if (response.status === 401) {
            throw new Error('Authentication failed. Check your API token.');
        }
        const error = await response.text();
        throw new Error(error || `HTTP ${response.status}`);
    }

    return response.json();
}

// Dashboard functions
async function loadDashboard() {
    try {
        const status = await apiRequest('/status');
        const groups = await apiRequest('/groups');
        const tasks = await apiRequest('/tasks');
        const chats = await apiRequest('/chats');

        // Update stats
        document.getElementById('stat-groups').textContent = groups.length;
        document.getElementById('stat-tasks').textContent = tasks.filter(t => t.status === 'active').length;
        document.getElementById('stat-chats').textContent = chats.length;
        document.getElementById('stat-uptime').textContent = status.uptime;

        // Update channels
        const channelsList = document.getElementById('channels-list');
        if (status.channels && status.channels.length > 0) {
            channelsList.innerHTML = status.channels.map(ch => `
                <div style="display: flex; justify-content: space-between; padding: 10px; border-bottom: 1px solid var(--border-color);">
                    <span>${ch.name}</span>
                    <span class="badge ${ch.connected ? 'badge-success' : 'badge-error'}">
                        ${ch.connected ? 'Connected' : 'Disconnected'}
                    </span>
                </div>
            `).join('');
        } else {
            channelsList.innerHTML = '<div class="empty-state">No channels configured</div>';
        }
    } catch (error) {
        console.error('Failed to load dashboard:', error);
        document.getElementById('channels-list').innerHTML = `
            <div class="alert alert-error">${error.message}</div>
        `;
    }
}

// Groups functions
async function loadGroups() {
    try {
        const groups = await apiRequest('/groups');
        const content = document.getElementById('groups-content');

        if (groups.length === 0) {
            content.innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-icon">📭</div>
                    <p>No groups registered yet</p>
                    <p style="margin-top: 10px;">Click "Register Group" to add one</p>
                </div>
            `;
            return;
        }

        content.innerHTML = `
            <table>
                <thead>
                    <tr>
                        <th>Name</th>
                        <th>JID</th>
                        <th>Folder</th>
                        <th>Trigger</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    ${groups.map(g => `
                        <tr>
                            <td>${escapeHtml(g.name)}</td>
                            <td><code>${escapeHtml(g.jid)}</code></td>
                            <td>${escapeHtml(g.folder)}</td>
                            <td>
                                <span class="badge ${g.requires_trigger ? 'badge-warning' : 'badge-success'}">
                                    ${g.requires_trigger ? 'Required' : 'Always On'}
                                </span>
                            </td>
                            <td>
                                <button class="btn btn-danger" onclick="unregisterGroup('${escapeHtml(g.jid)}')">
                                    Unregister
                                </button>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    } catch (error) {
        console.error('Failed to load groups:', error);
        document.getElementById('groups-content').innerHTML = `
            <div class="alert alert-error">${error.message}</div>
        `;
    }
}

async function registerGroup() {
    const jid = document.getElementById('register-jid').value.trim();
    const name = document.getElementById('register-name').value.trim();
    const requiresTrigger = document.getElementById('register-trigger').checked;

    if (!jid || !name) {
        alert('Please fill in all required fields');
        return;
    }

    try {
        await apiRequest('/groups', {
            method: 'POST',
            body: JSON.stringify({ jid, name, requires_trigger: requiresTrigger }),
        });
        closeRegisterModal();
        loadGroups();
        loadDashboard();
    } catch (error) {
        alert(`Failed to register group: ${error.message}`);
    }
}

async function unregisterGroup(jid) {
    if (!confirm(`Are you sure you want to unregister this group?`)) return;

    try {
        await apiRequest(`/groups/${encodeURIComponent(jid)}`, {
            method: 'DELETE',
        });
        loadGroups();
        loadDashboard();
    } catch (error) {
        alert(`Failed to unregister group: ${error.message}`);
    }
}

// Tasks functions
async function loadTasks() {
    try {
        const tasks = await apiRequest('/tasks');
        const content = document.getElementById('tasks-content');

        if (tasks.length === 0) {
            content.innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-icon">📋</div>
                    <p>No tasks scheduled</p>
                </div>
            `;
            return;
        }

        content.innerHTML = `
            <table>
                <thead>
                    <tr>
                        <th>ID</th>
                        <th>Group</th>
                        <th>Schedule</th>
                        <th>Status</th>
                        <th>Next Run</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    ${tasks.map(t => `
                        <tr>
                            <td><code>${escapeHtml(t.id.substring(0, 12))}...</code></td>
                            <td>${escapeHtml(t.group_folder)}</td>
                            <td><code>${escapeHtml(t.schedule_type)}: ${escapeHtml(t.schedule_value)}</code></td>
                            <td>
                                <span class="badge badge-${t.status === 'active' ? 'success' : t.status === 'paused' ? 'warning' : 'error'}">
                                    ${t.status}
                                </span>
                            </td>
                            <td>${t.next_run ? new Date(t.next_run).toLocaleString() : '-'}</td>
                            <td>
                                <button class="btn btn-primary" onclick="runTask('${escapeHtml(t.id)}')" style="padding: 6px 12px; font-size: 12px;">
                                    Run Now
                                </button>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    } catch (error) {
        console.error('Failed to load tasks:', error);
        document.getElementById('tasks-content').innerHTML = `
            <div class="alert alert-error">${error.message}</div>
        `;
    }
}

async function runTask(taskId) {
    try {
        await apiRequest(`/tasks/${taskId}/run`, { method: 'POST' });
        alert('Task triggered successfully');
        loadTasks();
    } catch (error) {
        alert(`Failed to run task: ${error.message}`);
    }
}

// Chats functions
async function loadChats() {
    try {
        const chats = await apiRequest('/chats');
        const content = document.getElementById('chats-content');

        if (chats.length === 0) {
            content.innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-icon">💬</div>
                    <p>No chats yet</p>
                </div>
            `;
            return;
        }

        content.innerHTML = `
            <table>
                <thead>
                    <tr>
                        <th>Name</th>
                        <th>JID</th>
                        <th>Channel</th>
                        <th>Type</th>
                        <th>Registered</th>
                        <th>Last Message</th>
                    </tr>
                </thead>
                <tbody>
                    ${chats.map(c => `
                        <tr>
                            <td>${escapeHtml(c.name)}</td>
                            <td><code>${escapeHtml(c.jid)}</code></td>
                            <td>${escapeHtml(c.channel)}</td>
                            <td>${c.is_group ? 'Group' : 'Direct'}</td>
                            <td>
                                <span class="badge ${c.is_registered ? 'badge-success' : 'badge-warning'}">
                                    ${c.is_registered ? 'Yes' : 'No'}
                                </span>
                            </td>
                            <td>${c.last_message_time ? new Date(c.last_message_time).toLocaleString() : 'Never'}</td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    } catch (error) {
        console.error('Failed to load chats:', error);
        document.getElementById('chats-content').innerHTML = `
            <div class="alert alert-error">${error.message}</div>
        `;
    }
}

// Settings functions
function saveSettings() {
    const host = document.getElementById('api-host').value;
    const token = document.getElementById('api-token').value;

    localStorage.setItem('omiga_api_host', host);
    localStorage.setItem('omiga_api_token', token);

    API_BASE = host.replace(/\/$/, '') + '/api';
    API_TOKEN = token;

    alert('Settings saved. Reloading...');
    location.reload();
}

// Modal functions
function showRegisterModal() {
    document.getElementById('register-modal').classList.add('active');
}

function closeRegisterModal() {
    document.getElementById('register-modal').classList.remove('active');
    document.getElementById('register-jid').value = '';
    document.getElementById('register-name').value = '';
    document.getElementById('register-trigger').checked = true;
}

// Utility functions
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Tab navigation
function setupTabs() {
    const tabs = document.querySelectorAll('.nav-tab');
    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            const tabName = tab.dataset.tab;

            // Remove active class from all tabs and content
            tabs.forEach(t => t.classList.remove('active'));
            document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));

            // Add active class to clicked tab
            tab.classList.add('active');
            document.getElementById(`${tabName}-tab`).classList.add('active');

            // Load content for the tab
            loadTabContent(tabName);
        });
    });
}

function loadTabContent(tabName) {
    switch (tabName) {
        case 'dashboard':
            loadDashboard();
            break;
        case 'groups':
            loadGroups();
            break;
        case 'tasks':
            loadTasks();
            break;
        case 'chats':
            loadChats();
            break;
    }
}

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    loadSettings();
    setupTabs();
    loadDashboard();

    // Auto-refresh dashboard every 30 seconds
    setInterval(loadDashboard, 30000);
});
